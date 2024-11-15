#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

pub mod configuration;
// pub mod kafka;
pub mod utils;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utils::{
  couchbase_db::{
    add_document as couchbase_add_document, delete_data as couchbase_delete_document,
    get_document as get_couchbase_document, get_documents as couchbase_get_documents,
    init_couchbase_connection, replace_document as couchbase_replace_document,
    get_documents_v2 as couchbase_get_documents_v2,
  },
  logger::LoggerConfig,
};

#[derive(Debug, Serialize, Deserialize)]
#[napi(object)]
pub struct BatchResponse {
  pub keys: Vec<String>,
  pub values: Option<Vec<Value>>,
}

// pub fn caste
#[derive(Debug)]
pub struct ReturnError {
  pub error: String,
}

#[napi]
pub fn startLogger() {
  // You can use handle to change logger config at runtime
  // just call startLogger() in main.rs and you can use log4rs in all your Project-crate.
  let Global_logs_config = LoggerConfig::create_Global_logs_config();
  let handle = log4rs::init_config(Global_logs_config).unwrap();
}

#[napi(js_name = "initCouchbase")]

pub fn init_couchbase() {
  init_couchbase_connection()
}

#[napi(js_name = "getDocuments")]
pub async fn get_documents(
  key: String,
  with_cas: bool,
  bucket_name: String,
) -> Result<Value, napi::Error> {
  let couchbase_data = get_couchbase_document(key.clone(), with_cas, bucket_name.clone()).await;
  match couchbase_data {
    Ok(cb_data) => {
      log::info!("Couchbase data: {:?}", cb_data);
      Ok(cb_data)
    }
    Err(error) => {
      log::error!("Error fetching document from Couchbase: {:?}", error);
      Err(napi::Error::from_reason(error.to_string()))
    }
  }
}

#[napi(js_name = "addDocument")]
pub async fn add_documents(
  key: String,
  value: Value,
  bucket_name: String,
) -> Result<bool, napi::Error> {
  match couchbase_add_document(key.clone(), value.clone(), bucket_name.clone(), Some(5)).await {
    Ok(cb_response) => {
      log::info!("Data successfully added to Couchbase for key: {}", key);
      Ok(cb_response)
    }
    Err(cb_error) => {
      log::error!("Failed to add document to Couchbase: {:?}", cb_error);
      Err(napi::Error::from_reason(format!(
        "Couchbase error: {}",
        cb_error
      )))
    }
  }
}

#[napi(js_name = "replaceDocument")]
pub async fn replace_documents(
  key: String,
  value: Value,
  #[napi(ts_arg_type = "bigint | null | undefined")] with_cas: Option<i64>,
  bucket_name: String,
) -> Result<bool, napi::Error> {
  // Use `with_cas` directly as an `Option<i64>`
  // convert this with_cas: Option<i64>  as option of u64
  let with_cas = with_cas.map(|x| x as u64);
  let cb_replace_response = couchbase_replace_document(
    key.clone(),
    value.clone(),
    with_cas,
    bucket_name.clone(),
    Some(5),
  )
  .await;

  match cb_replace_response {
    Ok(cb_replace_response) => {
      log::info!("Couchbase replace response: {:?}", cb_replace_response);
      Ok(true)
    }
    Err(error) => {
      log::error!("Error replacing document in Couchbase: {:?}", error);
      Err(napi::Error::from_reason(error.to_string()))
    }
  }
}

#[napi(js_name = "removeDocument")]

pub async fn remove_document(key: String, bucket_name: String) -> Result<String, napi::Error> {
  let cb_response = couchbase_delete_document(key.clone(), bucket_name.clone()).await;
  match cb_response {
    Ok(cb_response) => {
      log::info!("Couchbase response: {}", cb_response);
      Ok(cb_response)
    }
    Err(error) => {
      log::error!("Error deleting document from Couchbase: {:?}", error);
      Err(napi::Error::from_reason(error.to_string()))
    }
  }
}

#[napi(js_name = "getBatchDocuments")]

pub async fn couchbase_get_batchdocuments(
  keys: Vec<String>,
  with_cas: bool,
  bucket_name: String,
) -> Result<Value, napi::Error> {
  let cb_response = couchbase_get_documents(keys.clone(), with_cas, bucket_name.clone()).await;
  match cb_response {
    Ok(cb_response) => {
      log::info!("Couchbase batch response: {}", cb_response);
      Ok(cb_response)
    }
    Err(error) => {
      log::error!("Error deleting document from Couchbase: {:?}", error);
      Err(napi::Error::from_reason(error.to_string()))
    }
  }
}
#[napi(js_name = "getBatchDocumentsV2")]

pub async fn couchbase_get_batchdocuments_v2(
  keys: Vec<String>,
  with_cas: bool,
  bucket_name: String,
) -> Result<Value, napi::Error> {
  let cb_response = couchbase_get_documents_v2(keys.clone(), with_cas, bucket_name.clone()).await;
  match cb_response {
    Ok(cb_response) => {
      log::info!("Couchbase batch_v2 response: {}", cb_response);
      Ok(cb_response)
    }
    Err(error) => {
      log::error!("Error deleting document from Couchbase: {:?}", error);
      Err(napi::Error::from_reason(error.to_string()))
    }
  }
}
