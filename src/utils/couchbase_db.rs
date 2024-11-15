use couchbase::{Cluster, Collection, GetOptions, InsertOptions, RemoveOptions, ReplaceOptions, UpsertOptions};
use lazy_static::lazy_static;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    sync::RwLock,
    time::{self},
};
use uuid::Uuid;

use crate::configuration as config;
#[derive(serde::Serialize)]
pub struct Message<T> {
    status: u32,
    message_key: String,
    data: T,
}

pub struct CouchbaseConnParams {
    pub connection_url: String,
    pub username: String,
    pub password: String,
}

pub fn uuid() -> Uuid {
    Uuid::new_v4()
}

lazy_static! {
    static ref CB_CONNECTION: Arc<Cluster> = create_cluster_connection();
    static ref BUCKET_CONNECTIONS: RwLock<HashMap<String, Arc<Collection>>> = RwLock::new(HashMap::new());
    static ref OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
}

pub fn create_cluster_connection() -> Arc<Cluster> {
    // let cluster = Cluster::connect(config::get::<String>("couchbase.connectionurl"), config::get::<String>("couchbase.username"), config::get::<String>("couchbase.password"));
    let connection_url = std::env::var("COUCHBASE_CONNECTION_URL").expect("CONNECTION_URL must be set");
    let username = std::env::var("COUCHBASE_USERNAME").expect("USERNAME must be set");
    let password = std::env::var("COUCHBASE_PASSWORD").expect("PASSWORD must be set");
    let cluster = Cluster::connect(connection_url, username, password);
    
    Arc::new(cluster)
}

pub fn init_couchbase_connection() {
    let _ = CB_CONNECTION.clone();
}

pub async fn get_bucket_connection(bucket_name: String) -> Result<Arc<Collection>, String> {
    // Ensure we initialize the Couchbase connection first
    // let init_cb = init_couchbase_connection(None).await;
    // if let Err(err) = init_cb {
    //     return Err(format!("Failed to initialize Couchbase connection: {}", err));
    // }

    // Try to get the connection from the map
    if let Some(collection) = BUCKET_CONNECTIONS.read().await.get(&bucket_name) {
        // Connection already exists, so reuse it
        return Ok(Arc::clone(collection));
    }

    // If the connection doesn't exist, acquire a write lock to add it
    log::info!("Creating new connection for bucket: {}", bucket_name);

    // Check if the connection is still available
    // let cluster = CB_CONNECTION.get().ok_or_else(|| "No connection to cluster available".to_string())?;

    let bucket = CB_CONNECTION.bucket(&bucket_name);
    let collection = Arc::new(bucket.default_collection());

    // Insert the new connection into the map, ensuring only one write operation is done
    BUCKET_CONNECTIONS.write().await.insert(bucket_name, Arc::clone(&collection));

    Ok(collection)
}

// pub async fn get_bucket_connection(bucket_name: String) -> Result<Collection, String> {
//     // First try to initialzie connection with cluster
//     let init=init_couchbase_connection(None).await;

//     let bucket = cluster.bucket(bucket_name.clone());
//     let collection = bucket.default_collection();

//     Ok(collection)
// }

pub async fn get_document(
  key: String,
  with_cas: bool,
  bucket_name: String,
) -> Result<Value, String> {
  let db = get_bucket_connection(bucket_name).await;
  if let Err(err) = db {
    return Err(err);
  }
  let db = db.unwrap();

  match db.get(key.to_owned(), GetOptions::default()).await {
    Ok(get_result) => {
      let mut data = get_result.content::<Value>().unwrap();
      if with_cas {
        data = json!({
            "value":data,
            "cas":get_result.cas().to_string()
        });
      }
      Ok(data)
    }
    Err(error) => {
      log::error!(
        "Error in getting data from couchbase : {:?}",
        error.to_string()
      );
      Err(error.to_string())
    }
  }
}

pub async fn add_document(
  key: String,
  value: Value,
  bucket_name: String,
  retry: Option<u32>,
) -> Result<bool, String> {
  let retry = retry.unwrap_or(5);
  let db = get_bucket_connection(bucket_name.to_owned()).await;
  if let Err(err) = db {
    return Err(err);
  }
  let db = db.unwrap();

  match db
    .insert(key.clone(), value.to_owned(), InsertOptions::default())
    .await
  {
    Ok(_) => {
      // log::info!("Data successfully added to couchbase for key: {}", key);
      Ok(true)
    }
    Err(error) => {
      if retry <= 0 {
        return Err(format!(
          "Error in adding data to couchbase : {:?}... retry limit reached",
          error.to_string()
        ));
      }
      log::error!(
        "Error in adding data to couchbase : {:?}... retrying",
        error.to_string()
      );
      time::sleep(Duration::from_secs(1)).await;
      let res = Box::pin(add_document(key, value, bucket_name, Some(retry - 1))).await;
      if res.is_ok() {
        return Ok(true);
      }
      Err(error.to_string())
    }
  }
}

pub async fn replace_document(
  key: String,
  value: Value,
  cas: Option<u64>,
  bucket_name: String,
  retry: Option<u32>,
) -> Result<String, String> {
  let retry = retry.unwrap_or(5);
  let db = get_bucket_connection(bucket_name.to_owned()).await;
  if let Err(err) = db {
    return Err(err);
  }
  let db = db.unwrap();
  // let get_document_res = match db.get(key.clone(), GetOptions::default()).await {
  //     Ok(data) => data,
  //     Err(err) => {
  //         log::error!(
  //             "Error in getting data from couchbase : {:?}",
  //             err.to_string()
  //         );
  //         return Err(err.to_string());
  //     }
  // };
  // let cas = get_document_res.cas();
  let replace_opt;
  if cas.is_some() {
    replace_opt = ReplaceOptions::default().cas(cas.unwrap());
  } else {
    replace_opt = ReplaceOptions::default();
  }
  let update_data = db.replace(key.to_owned(), value.to_owned(), replace_opt);
  match update_data.await {
    Ok(_) => {
      // log::info!(
      //     "Data successfully updated to couchbase for key: {} in bucket : {}",
      //     key,
      //     bucket_name.to_owned()
      // );
      Ok(format!(
        "Data successfully updated to couchbase for key: {} in bucket : {}",
        key,
        bucket_name.to_owned()
      ))
    }
    Err(error) => {
      if retry <= 0 {
        return Err(format!(
          "Error in updating data to couchbase : {:?}... retry limit reached",
          error.to_string()
        ));
      }
      log::error!(
        "Error in updating data to couchbase : {:?} in bucket : {}",
        error.to_string(),
        bucket_name
      );
      time::sleep(Duration::from_secs(1)).await;
      let res = Box::pin(replace_document(
        key.to_owned(),
        value,
        cas,
        bucket_name.to_owned(),
        Some(retry - 1),
      ))
      .await;
      if res.is_ok() {
        return Ok(format!(
          "Data successfully updated to couchbase for key: {} in bucket : {}",
          key,
          bucket_name.to_owned()
        ));
      }
      Err(error.to_string())
    }
  }
}

pub async fn delete_data(key: String, bucket_name: String) -> Result<String, String> {
  let db = get_bucket_connection(bucket_name.to_owned()).await;
  if let Err(err) = db {
    return Err(err);
  }
  let db = db.unwrap();

  let delete_data = db.remove(key.to_owned(), RemoveOptions::default());
  match delete_data.await {
    Ok(_) => {
      // log::info!(
      //     "Data successfully deleted from couchbase for key: {} in bucket : {}",
      //     key,
      //     bucket_name.to_owned()
      // );
      Ok(format!(
        "Data successfully deleted from couchbase for key: {} in bucket : {}",
        key,
        bucket_name.to_owned()
      ))
    }
    Err(error) => {
      log::error!(
        "Error in deleting data from couchbase : {:?} in bucket : {}",
        error.to_string(),
        bucket_name
      );
      Err(error.to_string())
    }
  }
}


pub async fn get_documents(keys: Vec<String>, with_cas: bool, bucket_name: String) -> Result<Value, String> {
    let db = get_bucket_connection(bucket_name).await;
    if let Err(err) = db {
        return Err(format!("Error in getting bucket connection : {:?}", err));
    }
    let db = db.unwrap();

    if keys.is_empty() {
        return Err("Array of Keys need to be on length>0".to_string());
    }

    let mut docs: HashMap<String, Value> = HashMap::new();
    let mut errors: HashMap<String, Value> = HashMap::new();

    // Loop through each key
    for key in &keys {
        match db.get(key, GetOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
            Ok(res) => {
                let data = res.content::<Value>().unwrap();

                if with_cas {
                    docs.insert(
                        key.to_string(),
                        json!({
                            "value": data,
                            "cas": res.cas()
                        }),
                    );
                } else {
                    docs.insert(key.to_string(), json!(data));
                }
            }
            Err(err) => {
                errors.insert(
                    key.to_string(),
                    json!({
                        "error": err.to_string()
                    }),
                );
            }
        }
    }
    if errors.is_empty() {
        log::info!("All documents fetched successfully");
        Ok(json!(docs))
    } else {
        log::error!("Some documents failed to fetch");
        return Err(format!("Error occured while fetching documents {:?} :  {:?}", keys, errors));
    }
}

pub async fn get_documents_v2(keys: Vec<String>, with_cas: bool, bucket_name: String) -> Result<Value, String> {
    let db = get_bucket_connection(bucket_name).await;
    if let Err(err) = db {
        return Err(format!("Error in getting bucket connection : {:?}", err));
    }
    let db = db.unwrap();

    if keys.is_empty() {
        return Err("Array of Keys need to be on length>0".to_string());
    }

    let mut docs: HashMap<String, Value> = HashMap::new();
    let mut errors: HashMap<String, Value> = HashMap::new();

    // Loop through each key
    for key in &keys {
        match db.get(key, GetOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
            Ok(res) => {
                let data = res.content::<Value>().unwrap();

                if with_cas {
                    docs.insert(
                        key.to_string(),
                        json!({
                            "value": data,
                            "cas": res.cas()
                        }),
                    );
                } else {
                    docs.insert(key.to_string(), json!(data));
                }
            }
            Err(err) => {
                errors.insert(
                    key.to_string(),
                    json!({
                        "error": err.to_string()
                    }),
                );
            }
        }
    }
    Ok(json!({
        "docs":docs,
        "errors":errors
    }))
}

pub async fn get_next_counter_key(bucket_name: String, key: String, initial_counter: Option<u32>) -> Result<String, String> {
    // Try to get existing document
    let db = get_bucket_connection(bucket_name).await;
    if let Err(err) = db {
        return Err(format!("Error in getting bucket connection : {:?}", err));
    }
    let db = db.unwrap();

    let get_result = db.get(&key, GetOptions::default().timeout(OPERATION_TIMEOUT.clone())).await;

    match get_result {
        Ok(doc) => {
            // Document exists
            if let Some(initial) = initial_counter {
                // If initial counter provided, set it
                let value = Value::Number(initial.into());
                match db.upsert(&key, &value, UpsertOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
                    Ok(_) => {
                        log::info!("Initial counter set to {}", initial);
                    }
                    Err(err) => {
                        log::error!("Error in setting initial counter : {:?}", err);
                        return Err(err.to_string());
                    }
                };
                Ok(initial.to_string())
            } else {
                // Extract current counter value
                let counter = if let Ok(num) = doc.content::<i64>() {
                    num
                } else if let Ok(str_val) = doc.content::<String>() {
                    str_val.parse::<i64>().unwrap_or(0)
                } else {
                    return Err("Invalid counter format".to_string());
                };

                // Increment counter
                let new_counter = counter + 1;
                let value = Value::Number(new_counter.into());

                // Update document
                match db.upsert(&key, &value, UpsertOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
                    Ok(_) => {
                        log::info!("Counter incremented to {}", new_counter);
                    }
                    Err(err) => {
                        log::error!("Error in incrementing counter : {:?}", err);
                        return Err(err.to_string());
                    }
                };
                Ok(new_counter.to_string())
            }
        }
        Err(_) => {
            // Document doesn't exist, create with initial value
            let counter = initial_counter.unwrap_or(1) as i64;
            let value = Value::Number(counter.into());

            match db.upsert(&key, &value, UpsertOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
                Ok(_) => {
                    log::info!("Initial counter set to {}", counter);
                }
                Err(err) => {
                    log::error!("Error in setting initial counter : {:?}", err);
                    return Err(err.to_string());
                }
            };
            Ok(counter.to_string())
        }
    }
}

pub fn get_next_key() -> String {
  Uuid::new_v4().to_string()
}














//------------------------------------------------------


// use couchbase::{Cluster, Collection, GetOptions, InsertOptions, RemoveOptions, ReplaceOptions, UpsertOptions};
// use lazy_static::lazy_static;
// use serde_json::{json, Value};
// use std::{collections::HashMap, sync::Arc, time::Duration};
// use tokio::{
//     sync::RwLock,
//     time::{self},
// };
// use uuid::Uuid;

// use crate::configuration as config;
// #[derive(serde::Serialize)]
// pub struct Message<T> {
//     status: u32,
//     message_key: String,
//     data: T,
// }

// pub struct CouchbaseConnParams {
//     pub connection_url: String,
//     pub username: String,
//     pub password: String,
// }

// pub fn uuid() -> Uuid {
//     Uuid::new_v4()
// }

// lazy_static! {
//     static ref CB_CONNECTION: Arc<Cluster> = create_cluster_connection();
//     static ref BUCKET_CONNECTIONS: RwLock<HashMap<String, Arc<Collection>>> = RwLock::new(HashMap::new());
//     static ref OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
// }

// pub fn create_cluster_connection() -> Arc<Cluster> {
//     // let cluster = Cluster::connect(config::get::<String>("couchbase.connectionurl"), config::get::<String>("couchbase.username"), config::get::<String>("couchbase.password"));
//     let connection_url = std::env::var("CONNECTION_URL").expect("CONNECTION_URL must be set");
//     let username = std::env::var("USERNAME").expect("USERNAME must be set");
//     let password = std::env::var("PASSWORD").expect("PASSWORD must be set");
//     let cluster = Cluster::connect(connection_url, username, password);
    
//     Arc::new(cluster)
// }

// pub fn init_couchbase_connection() {
//     let _ = CB_CONNECTION.clone();
// }

// pub async fn get_bucket_connection(bucket_name: String) -> Result<Arc<Collection>, String> {
//     // Ensure we initialize the Couchbase connection first
//     // let init_cb = init_couchbase_connection(None).await;
//     // if let Err(err) = init_cb {
//     //     return Err(format!("Failed to initialize Couchbase connection: {}", err));
//     // }

//     // Try to get the connection from the map
//     if let Some(collection) = BUCKET_CONNECTIONS.read().await.get(&bucket_name) {
//         // Connection already exists, so reuse it
//         return Ok(Arc::clone(collection));
//     }

//     // If the connection doesn't exist, acquire a write lock to add it
//     log::info!("Creating new connection for bucket: {}", bucket_name);

//     // Check if the connection is still available
//     // let cluster = CB_CONNECTION.get().ok_or_else(|| "No connection to cluster available".to_string())?;

//     let bucket = CB_CONNECTION.bucket(&bucket_name);
//     let collection = Arc::new(bucket.default_collection());

//     // Insert the new connection into the map, ensuring only one write operation is done
//     BUCKET_CONNECTIONS.write().await.insert(bucket_name, Arc::clone(&collection));

//     Ok(collection)
// }

// // pub async fn get_bucket_connection(bucket_name: String) -> Result<Collection, String> {
// //     // First try to initialzie connection with cluster
// //     let init=init_couchbase_connection(None).await;

// //     let bucket = cluster.bucket(bucket_name.clone());
// //     let collection = bucket.default_collection();

// //     Ok(collection)
// // }

// pub async fn get_document(key: String, with_cas: bool, bucket_name: String,retry: Option<u32>) -> Result<Value, String> {
//     let db = get_bucket_connection(bucket_name.to_owned()).await;
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();
//     let retry = retry.unwrap_or(5);

//     match db.get(key.to_owned(), GetOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
//         Ok(get_result) => {
//             let mut data = get_result.content::<Value>().unwrap();
//             if with_cas {
//                 data = json!({
//                     "value":data,
//                     "cas":get_result.cas().to_string()
//                 });
//             }
//             Ok(data)
//         }
//         Err(error) => {
//             log::error!("Error in getting data from couchbase : {:?}", error.to_string());
//             if retry <= 0 {
//                 return Err(format!("Error in adding data to couchbase : {:?}... retry limit reached", error.to_string()));
//             }
//             time::sleep(Duration::from_secs(1)).await;
//             let res = Box::pin(get_document(key, with_cas, bucket_name, Some(retry - 1))).await;
//             if let Ok(data) = res {
//                 return Ok(data);
//             }
//             Err(error.to_string())
//         }
//     }
// }

// pub async fn add_document(key: String, value: Value, bucket_name: String, retry: Option<u32>) -> Result<bool, String> {
//     let db = get_bucket_connection(bucket_name.to_owned()).await;
//     let retry = retry.unwrap_or(5);
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();

//     match db.insert(key.clone(), value.to_owned(), InsertOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
//         Ok(_) => {
//             // log::info!("Data successfully added to couchbase for key: {}", key);
//             Ok(true)
//         }
//         Err(error) => {
//             log::error!("Error in adding data to couchbase : {:?}", error.to_string());
//             if retry <= 0 {
//                 return Err(format!("Error in adding data to couchbase : {:?}... retry limit reached", error.to_string()));
//             }
//             time::sleep(Duration::from_secs(1)).await;
//             let res = Box::pin(add_document(key, value, bucket_name, Some(retry - 1))).await;
//             if res.is_ok() {
//                 return Ok(true);
//             }
//             Err(error.to_string())
//         }
//     }
// }

// pub async fn add_document_with_ttl(key: String, value: Value, bucket_name: String, ttl:u64,  retry: Option<u32>) -> Result<bool, String> {
//     let retry = retry.unwrap_or(5);
//     let db = get_bucket_connection(bucket_name.to_owned()).await;
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();
//     let ttl=Duration::from_secs(ttl);
//     match db.insert(key.clone(), value.to_owned(), InsertOptions::default().timeout(OPERATION_TIMEOUT.clone()).expiry(ttl)).await {
//         Ok(_) => {
//             // log::info!("Data successfully added to couchbase for key: {}", key);
//             Ok(true)
//         }
//         Err(error) => {
//             log::error!("Error in adding data to couchbase : {:?}", error.to_string());
//             if retry <= 0 {
//                 return Err(format!("Error in adding data to couchbase : {:?}... retry limit reached", error.to_string()));
//             }
//             time::sleep(Duration::from_secs(1)).await;
//             let res = Box::pin(add_document(key, value, bucket_name, Some(retry - 1))).await;
//             if res.is_ok() {
//                 return Ok(true);
//             }
//             Err(error.to_string())
//         }
//     }
// }



// pub async fn replace_document(key: String, value: Value, cas: Option<u64>, bucket_name: String, retry: Option<u32>) -> Result<String, String> {
//     let retry = retry.unwrap_or(5);
//     let db = get_bucket_connection(bucket_name.to_owned()).await;
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();
//     // let get_document_res = match db.get(key.clone(), GetOptions::default()).await {
//     //     Ok(data) => data,
//     //     Err(err) => {
//     //         log::error!(
//     //             "Error in getting data from couchbase : {:?}",
//     //             err.to_string()
//     //         );
//     //         return Err(err.to_string());
//     //     }
//     // };
//     // let cas = get_document_res.cas();
//     let replace_opt;
//     if cas.is_some() {
//         replace_opt = ReplaceOptions::default().cas(cas.unwrap());
//     } else {
//         replace_opt = ReplaceOptions::default();
//     }
//     let update_data = db.replace(key.to_owned(), value.to_owned(), replace_opt.timeout(OPERATION_TIMEOUT.clone()));
//     match update_data.await {
//         Ok(_) => {
//             // log::info!(
//             //     "Data successfully updated to couchbase for key: {} in bucket : {}",
//             //     key,
//             //     bucket_name.to_owned()
//             // );
//             Ok(format!("Data successfully updated to couchbase for key: {} in bucket : {}", key, bucket_name.to_owned()))
//         }
//         Err(error) => {
//             log::error!("Error in updating data to couchbase : {:?} in bucket : {}", error.to_string(), bucket_name);
//             if retry <= 0 {
//                 return Err(format!("Error in updating data to couchbase : {:?}... retry limit reached", error.to_string()));
//             }
//             time::sleep(Duration::from_secs(1)).await;
//             let res = Box::pin(replace_document(key.to_owned(), value, cas, bucket_name.to_owned(), Some(retry - 1))).await;
//             if res.is_ok() {
//                 return Ok(format!("Data successfully updated to couchbase for key: {} in bucket : {}", key, bucket_name.to_owned()));
//             }
//             Err(error.to_string())
//         }
//     }
// }

// pub async fn delete_document(key: String, bucket_name: String) -> Result<String, String> {
//     let db = get_bucket_connection(bucket_name.to_owned()).await;
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();

//     let delete_document = db.remove(key.to_owned(), RemoveOptions::default().timeout(OPERATION_TIMEOUT.clone()));
//     match delete_document.await {
//         Ok(_) => {
//             // log::info!(
//             //     "Data successfully deleted from couchbase for key: {} in bucket : {}",
//             //     key,
//             //     bucket_name.to_owned()
//             // );
//             Ok(format!("Data successfully deleted from couchbase for key: {} in bucket : {}", key, bucket_name.to_owned()))
//         }
//         Err(error) => {
//             log::error!("Error in deleting data from couchbase : {:?} in bucket : {}", error.to_string(), bucket_name);
//             Err(error.to_string())
//         }
//     }
// }

// // pub async fn get_batch_using_scan(
// //   start: String,
// //   end: String,
// //   batch_size: i32,
// //   keysOnly: bool,
// //   bucket_name: String,
// // ){

// // }

// pub async fn get_documents(keys: Vec<String>, with_cas: bool, bucket_name: String) -> Result<Value, String> {
//     let db = get_bucket_connection(bucket_name).await;
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();

//     if keys.is_empty() {
//         return Err("Array of Keys need to be on length>0".to_string());
//     }

//     let mut docs: HashMap<String, Value> = HashMap::new();
//     let mut errors: HashMap<String, Value> = HashMap::new();

//     // Loop through each key
//     for key in &keys {
//         match db.get(key, GetOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
//             Ok(res) => {
//                 let data = res.content::<Value>().unwrap();

//                 if with_cas {
//                     docs.insert(
//                         key.to_string(),
//                         json!({
//                             "value": data,
//                             "cas": res.cas()
//                         }),
//                     );
//                 } else {
//                     docs.insert(key.to_string(), json!(data));
//                 }
//             }
//             Err(err) => {
//                 errors.insert(
//                     key.to_string(),
//                     json!({
//                         "error": err.to_string()
//                     }),
//                 );
//             }
//         }
//     }
//     if errors.is_empty() {
//         log::info!("All documents fetched successfully");
//         Ok(json!(docs))
//     } else {
//         log::error!("Some documents failed to fetch");
//         return Err(format!("Error occured while fetching documents {:?} :  {:?}", keys, errors));
//     }
// }

// pub async fn get_documents_v2(keys: Vec<String>, with_cas: bool, bucket_name: String) -> Result<Value, String> {
//     let db = get_bucket_connection(bucket_name).await;
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();

//     if keys.is_empty() {
//         return Err("Array of Keys need to be on length>0".to_string());
//     }

//     let mut docs: HashMap<String, Value> = HashMap::new();
//     let mut errors: HashMap<String, Value> = HashMap::new();

//     // Loop through each key
//     for key in &keys {
//         match db.get(key, GetOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
//             Ok(res) => {
//                 let data = res.content::<Value>().unwrap();

//                 if with_cas {
//                     docs.insert(
//                         key.to_string(),
//                         json!({
//                             "value": data,
//                             "cas": res.cas()
//                         }),
//                     );
//                 } else {
//                     docs.insert(key.to_string(), json!(data));
//                 }
//             }
//             Err(err) => {
//                 errors.insert(
//                     key.to_string(),
//                     json!({
//                         "error": err.to_string()
//                     }),
//                 );
//             }
//         }
//     }
//     Ok(json!({
//         "docs":docs,
//         "errors":errors
//     }))
// }

// pub async fn get_next_counter_key(bucket_name: String, key: String, initial_counter: Option<u32>) -> Result<String, String> {
//     // Try to get existing document
//     let db = get_bucket_connection(bucket_name).await;
//     if let Err(err) = db {
//         return Err(format!("Error in getting bucket connection : {:?}", err));
//     }
//     let db = db.unwrap();

//     let get_result = db.get(&key, GetOptions::default().timeout(OPERATION_TIMEOUT.clone())).await;

//     match get_result {
//         Ok(doc) => {
//             // Document exists
//             if let Some(initial) = initial_counter {
//                 // If initial counter provided, set it
//                 let value = Value::Number(initial.into());
//                 match db.upsert(&key, &value, UpsertOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
//                     Ok(_) => {
//                         log::info!("Initial counter set to {}", initial);
//                     }
//                     Err(err) => {
//                         log::error!("Error in setting initial counter : {:?}", err);
//                         return Err(err.to_string());
//                     }
//                 };
//                 Ok(initial.to_string())
//             } else {
//                 // Extract current counter value
//                 let counter = if let Ok(num) = doc.content::<i64>() {
//                     num
//                 } else if let Ok(str_val) = doc.content::<String>() {
//                     str_val.parse::<i64>().unwrap_or(0)
//                 } else {
//                     return Err("Invalid counter format".to_string());
//                 };

//                 // Increment counter
//                 let new_counter = counter + 1;
//                 let value = Value::Number(new_counter.into());

//                 // Update document
//                 match db.upsert(&key, &value, UpsertOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
//                     Ok(_) => {
//                         log::info!("Counter incremented to {}", new_counter);
//                     }
//                     Err(err) => {
//                         log::error!("Error in incrementing counter : {:?}", err);
//                         return Err(err.to_string());
//                     }
//                 };
//                 Ok(new_counter.to_string())
//             }
//         }
//         Err(_) => {
//             // Document doesn't exist, create with initial value
//             let counter = initial_counter.unwrap_or(1) as i64;
//             let value = Value::Number(counter.into());

//             match db.upsert(&key, &value, UpsertOptions::default().timeout(OPERATION_TIMEOUT.clone())).await {
//                 Ok(_) => {
//                     log::info!("Initial counter set to {}", counter);
//                 }
//                 Err(err) => {
//                     log::error!("Error in setting initial counter : {:?}", err);
//                     return Err(err.to_string());
//                 }
//             };
//             Ok(counter.to_string())
//         }
//     }
// }

// // pub async fn get_documents_from_view(
// //     cluster: &Cluster,
// //     design_doc: &str,
// //     view_name: &str,
// //     query_key: Option<Value>,
// //     bucket_name: &str,
// // ) -> Result<Vec<Value>, String> {
// //     // Get bucket reference
// //     let bucket = cluster.bucket(bucket_name);
    
// //     // Create view query
// //     let mut view_query = ViewQuery::from(design_doc, view_name);
    
// //     // Add key if provided
// //     if let Some(key) = query_key {
// //         view_query = view_query.key(key);
// //     }
    
// //     // Execute query
// //     let result = bucket
// //         .view_query(view_query)
// //         .await
// //         .map_err(|e| {
// //             eprintln!("Error occurred during view query: {:?}", e);
// //             e
// //         })?;
    
// //     // Collect results into a vector
// //     let mut docs = Vec::new();
// //     let mut rows = result.rows();
    
// //     while let Some(row) = rows.next().await {
// //         match row {
// //             Ok(doc) => docs.push(doc),
// //             Err(e) => {
// //                 eprintln!("Error processing row: {:?}", e);
// //                 return Err(Box::new(e));
// //             }
// //         }
// //     }
    
// //     Ok(docs)
// // }