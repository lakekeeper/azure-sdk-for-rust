use azure_core::error::{ErrorKind, ResultExt};
use azure_storage::prelude::StorageCredentials;
use azure_storage_datalake::prelude::*;
use std::future::Future;

/// Response variants recorded for each datalake transaction. The same request
/// fixtures are replayed against each variant so tests cover both
/// "server header present" and "server header missing" scenarios.
#[allow(dead_code)]
pub const RESPONSE_VARIANTS: &[&str] = &["with_server", "without_server"];

#[allow(dead_code)]
pub async fn create_data_lake_client(
    transaction_name: &str,
    response_variant: &str,
) -> azure_core::Result<DataLakeClient> {
    let account_name = (std::env::var(mock_transport::TESTING_MODE_KEY).as_deref()
        == Ok(mock_transport::TESTING_MODE_RECORD))
    .then(get_account)
    .unwrap_or_default();

    let account_key = (std::env::var(mock_transport::TESTING_MODE_KEY).as_deref()
        == Ok(mock_transport::TESTING_MODE_RECORD))
    .then(get_key)
    .unwrap_or_default();

    let transport_options = azure_core::TransportOptions::new_custom_policy(
        mock_transport::new_mock_transport_with_response_variant(
            transaction_name.into(),
            response_variant.into(),
        ),
    );

    let storage_credentials = StorageCredentials::access_key(account_name.clone(), account_key);
    Ok(DataLakeClient::builder(account_name, storage_credentials)
        .transport(transport_options)
        .build())
}

/// Run the given async test body once per entry in [`RESPONSE_VARIANTS`],
/// supplying a fresh `DataLakeClient` configured for that variant each time.
///
/// On failure, the variant name is attached to the returned error so it's
/// obvious which response set caused the regression.
#[allow(dead_code)]
pub async fn run_with_each_response_variant<F, Fut>(
    transaction_name: &str,
    body: F,
) -> azure_core::Result<()>
where
    F: Fn(DataLakeClient) -> Fut,
    Fut: Future<Output = azure_core::Result<()>>,
{
    for variant in RESPONSE_VARIANTS {
        let client = create_data_lake_client(transaction_name, variant)
            .await
            .unwrap();
        body(client).await.with_context(ErrorKind::Other, || {
            format!("variant '{variant}' failed for transaction '{transaction_name}'")
        })?;
    }
    Ok(())
}

fn get_account() -> String {
    std::env::var("ADLSGEN2_STORAGE_ACCOUNT")
        .expect("Set env variable ADLSGEN2_STORAGE_ACCOUNT first!")
}

fn get_key() -> String {
    std::env::var("ADLSGEN2_STORAGE_ACCESS_KEY")
        .expect("Set env variable ADLSGEN2_STORAGE_ACCESS_KEY first!")
}
