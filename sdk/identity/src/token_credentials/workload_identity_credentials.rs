use crate::{
    federated_credentials_flow, token_credentials::cache::TokenCache, TokenCredentialOptions,
};
use azure_core::{
    auth::{AccessToken, Secret, TokenCredential},
    error::{ErrorKind, ResultExt},
    Error, HttpClient,
};
use std::{path::PathBuf, str, sync::Arc, time::Duration};
use time::OffsetDateTime;
use url::Url;

const AZURE_TENANT_ID_ENV_KEY: &str = "AZURE_TENANT_ID";
const AZURE_CLIENT_ID_ENV_KEY: &str = "AZURE_CLIENT_ID";
const AZURE_FEDERATED_TOKEN_FILE: &str = "AZURE_FEDERATED_TOKEN_FILE";
const AZURE_FEDERATED_TOKEN: &str = "AZURE_FEDERATED_TOKEN";

/// Enables authentication to Azure Active Directory using a client secret that was generated for an App Registration.
///
/// More information on how to configure a client secret can be found here:
/// <https://docs.microsoft.com/azure/active-directory/develop/quickstart-configure-app-access-web-apis#add-credentials-to-your-web-application>

#[derive(Debug)]
pub struct WorkloadIdentityCredential {
    http_client: Arc<dyn HttpClient>,
    authority_host: Url,
    tenant_id: String,
    client_id: String,
    token: FederatedToken,
    cache: TokenCache,
}

/// Where the federated token sent to Azure Active Directory comes from.
#[derive(Debug)]
enum FederatedToken {
    /// A token that does not change.
    Fixed(Secret),
    /// A file that is read on every token request, because its token is rotated (for
    /// example the projected service account token of AKS workload identity).
    File(PathBuf),
}

impl FederatedToken {
    fn read(&self) -> azure_core::Result<Secret> {
        match self {
            Self::Fixed(token) => Ok(token.clone()),
            Self::File(path) => std::fs::read_to_string(path).map(Secret::new).with_context(
                ErrorKind::Credential,
                || {
                    format!(
                        "failed to read federated token from file {}",
                        path.display()
                    )
                },
            ),
        }
    }
}

impl WorkloadIdentityCredential {
    /// Create a new `WorkloadIdentityCredential`
    pub fn new<T>(
        http_client: Arc<dyn HttpClient>,
        authority_host: Url,
        tenant_id: String,
        client_id: String,
        token: T,
    ) -> Self
    where
        T: Into<Secret>,
    {
        Self {
            http_client,
            authority_host,
            tenant_id,
            client_id,
            token: FederatedToken::Fixed(token.into()),
            cache: TokenCache::new(),
        }
    }

    /// Create a new `WorkloadIdentityCredential` that reads the federated token from
    /// `token_file` on every token request.
    pub fn from_token_file(
        http_client: Arc<dyn HttpClient>,
        authority_host: Url,
        tenant_id: String,
        client_id: String,
        token_file: impl Into<PathBuf>,
    ) -> Self {
        Self {
            http_client,
            authority_host,
            tenant_id,
            client_id,
            token: FederatedToken::File(token_file.into()),
            cache: TokenCache::new(),
        }
    }

    pub fn create(
        options: impl Into<TokenCredentialOptions>,
    ) -> azure_core::Result<WorkloadIdentityCredential> {
        let options = options.into();
        let http_client = options.http_client();
        let authority_host = options.authority_host()?;
        let env = options.env();
        let tenant_id =
            env.var(AZURE_TENANT_ID_ENV_KEY)
                .with_context(ErrorKind::Credential, || {
                    format!(
                        "working identity credential requires {} environment variable",
                        AZURE_TENANT_ID_ENV_KEY
                    )
                })?;
        let client_id =
            env.var(AZURE_CLIENT_ID_ENV_KEY)
                .with_context(ErrorKind::Credential, || {
                    format!(
                        "working identity credential requires {} environment variable",
                        AZURE_CLIENT_ID_ENV_KEY
                    )
                })?;

        if let Ok(token) = env
            .var(AZURE_FEDERATED_TOKEN)
            .map_kind(ErrorKind::Credential)
        {
            return Ok(WorkloadIdentityCredential::new(
                http_client,
                authority_host,
                tenant_id,
                client_id,
                token,
            ));
        }

        if let Ok(token_file) = env
            .var(AZURE_FEDERATED_TOKEN_FILE)
            .map_kind(ErrorKind::Credential)
        {
            let credential = WorkloadIdentityCredential::from_token_file(
                http_client,
                authority_host,
                tenant_id,
                client_id,
                token_file,
            );
            // Fail on creation if the file cannot be read, so a credential chain moves on.
            credential.token.read()?;
            return Ok(credential);
        }

        Err(Error::with_message(ErrorKind::Credential, || {
            format!("working identity credential requires {AZURE_FEDERATED_TOKEN} or {AZURE_FEDERATED_TOKEN_FILE} environment variables")
        }))
    }

    async fn get_token(&self, scopes: &[&str]) -> azure_core::Result<AccessToken> {
        let token = self.token.read()?;
        let res: AccessToken = federated_credentials_flow::perform(
            self.http_client.clone(),
            &self.client_id,
            token.secret(),
            scopes,
            &self.tenant_id,
            &self.authority_host,
        )
        .await
        .map(|r| {
            AccessToken::new(
                r.access_token().clone(),
                OffsetDateTime::now_utc() + Duration::from_secs(r.expires_in),
            )
        })
        .context(ErrorKind::Credential, "request token error")?;
        Ok(res)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl TokenCredential for WorkloadIdentityCredential {
    async fn get_token(&self, scopes: &[&str]) -> azure_core::Result<AccessToken> {
        self.cache.get_token(scopes, self.get_token(scopes)).await
    }

    async fn clear_cache(&self) -> azure_core::Result<()> {
        self.cache.clear().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::Env;
    use azure_core::{headers::Headers, BytesStream, Request, Response, StatusCode};
    use std::sync::Mutex;

    /// Answers every token request with a token that is already expired, so every
    /// `get_token` reaches the token endpoint, and records the assertion it was sent.
    #[derive(Debug, Default)]
    struct RecordingTokenEndpoint {
        assertions: Mutex<Vec<String>>,
    }

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl HttpClient for RecordingTokenEndpoint {
        async fn execute_request(&self, request: &Request) -> azure_core::Result<Response> {
            let azure_core::Body::Bytes(body) = request.body() else {
                panic!("the token request has a bytes body");
            };
            let assertion = url::form_urlencoded::parse(body)
                .find(|(key, _)| key == "client_assertion")
                .map(|(_, value)| value.into_owned())
                .expect("the token request carries a client assertion");
            self.assertions.lock().unwrap().push(assertion);
            let body = r#"{"token_type":"Bearer","expires_in":0,"ext_expires_in":0,"access_token":"access"}"#;
            Ok(Response::new(
                StatusCode::Ok,
                Headers::new(),
                Box::pin(BytesStream::new(body)),
            ))
        }
    }

    fn token_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "azure-identity-{name}-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ))
    }

    fn credential(
        endpoint: &Arc<RecordingTokenEndpoint>,
        vars: &[(&str, &str)],
    ) -> WorkloadIdentityCredential {
        let mut vars = vars.to_vec();
        vars.extend([
            (AZURE_TENANT_ID_ENV_KEY, "tenant"),
            (AZURE_CLIENT_ID_ENV_KEY, "client"),
        ]);
        WorkloadIdentityCredential::create(TokenCredentialOptions::new(
            Env::from(&vars[..]),
            endpoint.clone(),
        ))
        .expect("the credential is created")
    }

    #[tokio::test]
    async fn sends_the_current_content_of_the_token_file() {
        let path = token_file("rotated");
        std::fs::write(&path, "first").unwrap();
        let endpoint = Arc::new(RecordingTokenEndpoint::default());
        let credential = credential(
            &endpoint,
            &[(AZURE_FEDERATED_TOKEN_FILE, path.to_str().unwrap())],
        );

        TokenCredential::get_token(&credential, &["scope"])
            .await
            .unwrap();
        std::fs::write(&path, "second").unwrap();
        TokenCredential::get_token(&credential, &["scope"])
            .await
            .unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(*endpoint.assertions.lock().unwrap(), ["first", "second"]);
    }

    #[tokio::test]
    async fn keeps_a_token_given_by_value() {
        let endpoint = Arc::new(RecordingTokenEndpoint::default());
        let credential = credential(&endpoint, &[(AZURE_FEDERATED_TOKEN, "fixed")]);

        TokenCredential::get_token(&credential, &["scope"])
            .await
            .unwrap();
        TokenCredential::get_token(&credential, &["scope"])
            .await
            .unwrap();

        assert_eq!(*endpoint.assertions.lock().unwrap(), ["fixed", "fixed"]);
    }

    #[test]
    fn fails_on_creation_if_the_token_file_is_missing() {
        let path = token_file("missing");
        let options = TokenCredentialOptions::new(
            Env::from(
                &[
                    (AZURE_TENANT_ID_ENV_KEY, "tenant"),
                    (AZURE_CLIENT_ID_ENV_KEY, "client"),
                    (AZURE_FEDERATED_TOKEN_FILE, path.to_str().unwrap()),
                ][..],
            ),
            Arc::new(RecordingTokenEndpoint::default()),
        );

        let error = WorkloadIdentityCredential::create(options)
            .expect_err("a missing token file fails the credential");
        assert_eq!(error.kind(), &ErrorKind::Credential);
    }
}
