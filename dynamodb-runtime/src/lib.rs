use anyhow::Result;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::{Client, Error};
use metashrew_runtime::{BatchLike, KeyValueStoreLike};
use std::sync::Arc;
use tokio::runtime::Runtime;

const TIP_HEIGHT_KEY: &'static str = "/__INTERNAL/tip-height";

pub struct DynamoDbRuntimeAdapter {
    client: Arc<Client>,
    table_name: String,
    current_height: u32,
}

static mut _LABEL: Option<String> = None;

// Keeping label functionality unchanged
pub fn set_label(s: String) -> () {
    unsafe {
        _LABEL = Some(s + "://");
    }
}

pub fn get_label() -> &'static String {
    unsafe { _LABEL.as_ref().unwrap() }
}

pub fn has_label() -> bool {
    unsafe { _LABEL.is_some() }
}

pub fn to_labeled_key(key: &Vec<u8>) -> Vec<u8> {
    if has_label() {
        let mut result: Vec<u8> = vec![];
        result.extend(get_label().as_str().as_bytes());
        result.extend(key);
        result
    } else {
        key.clone()
    }
}

pub async fn query_height(client: &Client, table_name: &str, start_block: u32) -> Result<u32> {
    let height_key = TIP_HEIGHT_KEY.as_bytes().to_vec();
    let labeled_key = to_labeled_key(&height_key);

    let result = client
        .get_item()
        .table_name(table_name)
        .key("pk", AttributeValue::B(labeled_key.into()))
        .send()
        .await?;

    match result.item {
        Some(item) => {
            if let Some(AttributeValue::B(bytes)) = item.get("value") {
                let bytes_ref: &[u8] = bytes.as_ref();
                Ok(u32::from_le_bytes(bytes_ref.try_into().unwrap()))
            } else {
                Ok(start_block)
            }
        }
        None => Ok(start_block),
    }
}

impl DynamoDbRuntimeAdapter {
    pub async fn new(table_name: String) -> Result<Self> {
        let region_provider = RegionProviderChain::default_provider();
        let config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&config);

        Ok(Self {
            client: Arc::new(client),
            table_name,
            current_height: 0,
        })
    }
}

pub struct DynamoDbBatch {
    items: Vec<(Vec<u8>, Vec<u8>)>,
}

impl BatchLike for DynamoDbBatch {
    fn default() -> Self {
        Self { items: Vec::new() }
    }

    fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) {
        self.items.push((k.as_ref().to_vec(), v.as_ref().to_vec()));
    }
}

impl Clone for DynamoDbRuntimeAdapter {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            table_name: self.table_name.clone(),
            current_height: self.current_height,
        }
    }
}

impl KeyValueStoreLike for DynamoDbRuntimeAdapter {
    type Batch = DynamoDbBatch;
    type Error = anyhow::Error;

    fn write(&mut self, batch: DynamoDbBatch) -> Result<(), Self::Error> {
        let rt = Runtime::new().map_err(Into::<anyhow::Error>::into)?;
        rt.block_on(async {
            let mut requests = Vec::new();

            for (key, value) in batch.items {
                let labeled_key = to_labeled_key(&key);
                requests.push(
                    aws_sdk_dynamodb::types::WriteRequest::builder()
                        .put_request(
                            aws_sdk_dynamodb::types::PutRequest::builder()
                                .item("pk", AttributeValue::B(labeled_key.into()))
                                .item("value", AttributeValue::B(value.into()))
                                .build()
                                .map_err(Into::<anyhow::Error>::into)?,
                        )
                        .build(),
                );
            }

            // Handle height update
            let height_key = TIP_HEIGHT_KEY.as_bytes().to_vec();
            let height_bytes = (self.current_height + 1).to_le_bytes().to_vec();
            requests.push(
                aws_sdk_dynamodb::types::WriteRequest::builder()
                    .put_request(
                        aws_sdk_dynamodb::types::PutRequest::builder()
                            .item("pk", AttributeValue::B(to_labeled_key(&height_key).into()))
                            .item("value", AttributeValue::B(height_bytes.into()))
                            .build()
                            .map_err(Into::<anyhow::Error>::into)?,
                    )
                    .build(),
            );
            self.client
                .batch_write_item()
                .request_items(self.table_name.clone(), requests)
                .send()
                .await
                .map_err(Into::<anyhow::Error>::into)?;

            self.current_height += 1;
            Ok(())
        })
    }

    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error> {
        let rt = Runtime::new().map_err(Into::<anyhow::Error>::into)?;
        rt.block_on(async {
            let labeled_key = to_labeled_key(&key.as_ref().to_vec());

            let result = self
                .client
                .get_item()
                .table_name(&self.table_name)
                .key("pk", AttributeValue::B(labeled_key.into()))
                .send()
                .await
                .map_err(Into::<anyhow::Error>::into)?;

            Ok(result.item.and_then(|item| {
                if let Some(AttributeValue::B(bytes)) = item.get("value") {
                    Some(bytes.as_ref().to_vec())
                } else {
                    None
                }
            }))
        })
    }

    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error> {
        let rt = Runtime::new().map_err(Into::<anyhow::Error>::into)?;
        rt.block_on(async {
            let labeled_key = to_labeled_key(&key.as_ref().to_vec());

            self.client
                .delete_item()
                .table_name(&self.table_name)
                .key("pk", AttributeValue::B(labeled_key.into()))
                .send()
                .await
                .map_err(Into::<anyhow::Error>::into)?;

            Ok(())
        })
    }

    fn put<K, V>(&self, key: K, value: V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let rt = Runtime::new().map_err(Into::<anyhow::Error>::into)?;
        rt.block_on(async {
            let labeled_key = to_labeled_key(&key.as_ref().to_vec());

            self.client
                .put_item()
                .table_name(&self.table_name)
                .item("pk", AttributeValue::B(labeled_key.into()))
                .item("value", AttributeValue::B(value.as_ref().to_vec().into()))
                .send()
                .await
                .map_err(Into::<anyhow::Error>::into)?;

            Ok(())
        })
    }
}
