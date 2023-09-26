use crate::{client::utils::generate_unique_registered_user, client_api_client};
use serde_json::json;

use collab_define::CollabType;
use sqlx::types::{uuid, Uuid};

use crate::realtime::test_client::{assert_collab_json, TestClient};

use assert_json_diff::assert_json_eq;
use collab::core::collab_state::SyncState;
use shared_entity::error_code::ErrorCode;
use std::time::Duration;
use storage::collab::FLUSH_PER_UPDATE;
use storage_entity::QueryCollabParams;
use tokio_stream::StreamExt;

#[tokio::test]
async fn realtime_write_single_collab_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new().await;
  test_client.create(&object_id, collab_type.clone()).await;

  let mut sync_state = test_client
    .collab_by_object_id
    .get(&object_id)
    .unwrap()
    .collab
    .lock()
    .subscribe_sync_state();

  // Edit the collab
  for i in 0..=5 {
    test_client
      .collab_by_object_id
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }

  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(4)) => panic!("sync timeout"),
       result = sync_state.next() => {
        match result {
          Some(new_state) => {
            if new_state == SyncState::SyncFinished {
              break;
            }
          },
          None => panic!("sync error"),
        }
       },
    }
  }

  assert_collab_json(
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    3,
    json!( {
      "0": "0",
      "1": "1",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
    }),
  )
  .await;
}

#[tokio::test]
async fn realtime_write_multiple_collab_test() {
  let mut test_client = TestClient::new().await;
  let mut object_ids = vec![];
  for _ in 0..10 {
    let object_id = uuid::Uuid::new_v4().to_string();
    let collab_type = CollabType::Document;
    test_client.create(&object_id, collab_type.clone()).await;
    for i in 0..=5 {
      test_client
        .collab_by_object_id
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert(&i.to_string(), i.to_string());
    }

    object_ids.push(object_id);
  }

  // Wait for the messages to be sent
  tokio::time::sleep(Duration::from_secs(2)).await;
  for object_id in object_ids {
    assert_collab_json(
      &mut test_client.api_client,
      &object_id,
      &CollabType::Document,
      3,
      json!( {
        "0": "0",
        "1": "1",
        "2": "2",
        "3": "3",
        "4": "4",
        "5": "5",
      }),
    )
    .await;
  }
}

#[tokio::test]
async fn one_direction_peer_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new().await;
  client_1.create(&object_id, collab_type.clone()).await;

  let mut client_2 = TestClient::new().await;
  client_2.create(&object_id, collab_type.clone()).await;

  // Edit the collab from client 1 and then the server will broadcast to client 2
  for _i in 0..=FLUSH_PER_UPDATE {
    client_1
      .collab_by_object_id
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert("name", "AppFlowy");
    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  assert_collab_json(
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    5,
    json!({
      "name": "AppFlowy"
    }),
  )
  .await;

  let json_1 = client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  let json_2 = client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  assert_json_eq!(json_1, json_2);
}

#[tokio::test]
async fn user_with_duplicate_devices_connect_edit_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let registered_user = generate_unique_registered_user().await;

  // Client_1_2 will force the server to disconnect client_1_1. So any changes made by client_1_1
  // will not be saved to the server.
  let device_id = Uuid::new_v4().to_string();
  let mut client_1_1 =
    TestClient::new_with_device_id(device_id.clone(), registered_user.clone()).await;
  client_1_1.create(&object_id, collab_type.clone()).await;

  client_1_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("1", "a");
  client_1_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("3", "c");
  tokio::time::sleep(Duration::from_millis(500)).await;

  let mut client_1_2 =
    TestClient::new_with_device_id(device_id.clone(), registered_user.clone()).await;
  client_1_2.create(&object_id, collab_type.clone()).await;
  client_1_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("2", "b");
  tokio::time::sleep(Duration::from_millis(500)).await;

  let json_1 = client_1_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  let json_2 = client_1_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  assert_json_eq!(
    json_1,
    json!({
      "1": "a",
      "3": "c"
    })
  );
  assert_json_eq!(
    json_2,
    json!({
      "1": "a",
      "3": "c",
      "2": "b"
    })
  );
  assert_collab_json(
    &mut client_1_2.api_client,
    &object_id,
    &collab_type,
    5,
    json!({
      "1": "a",
      "2": "b",
      "3": "c"
    }),
  )
  .await;
}

#[tokio::test]
async fn two_direction_peer_sync_test() {
  let _client_api = client_api_client();
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new().await;
  client_1.create(&object_id, collab_type.clone()).await;

  let mut client_2 = TestClient::new().await;
  client_2.create(&object_id, collab_type.clone()).await;

  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  tokio::time::sleep(Duration::from_millis(10)).await;

  client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("support platform", "macOS, Windows, Linux, iOS, Android");
  tokio::time::sleep(Duration::from_millis(1000)).await;

  let json_1 = client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  let json_2 = client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  assert_json_eq!(
    json_1,
    json!({
      "name": "AppFlowy",
      "support platform": "macOS, Windows, Linux, iOS, Android"
    })
  );
  assert_json_eq!(json_1, json_2);
}

#[tokio::test]
async fn client_init_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new().await;
  client_1.create(&object_id, collab_type.clone()).await;
  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  tokio::time::sleep(Duration::from_millis(10)).await;

  let mut client_2 = TestClient::new().await;
  client_2.create(&object_id, collab_type.clone()).await;
  tokio::time::sleep(Duration::from_millis(1000)).await;

  // Open the collab from client 2. After the initial sync, the server will send the missing updates to client_2.
  let json_1 = client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  let json_2 = client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  assert_json_eq!(
    json_1,
    json!({
      "name": "AppFlowy",
    })
  );
  assert_json_eq!(json_1, json_2);
}

#[tokio::test]
async fn multiple_collab_edit_test() {
  let collab_type = CollabType::Document;
  let object_id_1 = uuid::Uuid::new_v4().to_string();
  let mut client_1 = TestClient::new().await;
  client_1.create(&object_id_1, collab_type.clone()).await;

  let object_id_2 = uuid::Uuid::new_v4().to_string();
  let mut client_2 = TestClient::new().await;
  client_2.create(&object_id_2, collab_type.clone()).await;

  let object_id_3 = uuid::Uuid::new_v4().to_string();
  let mut client_3 = TestClient::new().await;
  client_3.create(&object_id_3, collab_type.clone()).await;

  client_1
    .collab_by_object_id
    .get_mut(&object_id_1)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 1");
  client_2
    .collab_by_object_id
    .get_mut(&object_id_2)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 2");
  client_3
    .collab_by_object_id
    .get_mut(&object_id_3)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 3");
  tokio::time::sleep(Duration::from_secs(2)).await;

  assert_collab_json(
    &mut client_1.api_client,
    &object_id_1,
    &collab_type,
    3,
    json!( {
      "title": "I am client 1"
    }),
  )
  .await;

  assert_collab_json(
    &mut client_2.api_client,
    &object_id_2,
    &collab_type,
    3,
    json!( {
      "title": "I am client 2"
    }),
  )
  .await;
  assert_collab_json(
    &mut client_3.api_client,
    &object_id_3,
    &collab_type,
    3,
    json!( {
      "title": "I am client 3"
    }),
  )
  .await;
}

#[tokio::test]
async fn ws_reconnect_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut test_client = TestClient::new().await;
  test_client.create(&object_id, collab_type.clone()).await;

  // Disconnect the client and edit the collab. The updates will not be sent to the server.
  test_client.disconnect().await;
  for i in 0..=5 {
    test_client
      .collab_by_object_id
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }

  // it will return RecordNotFound error when trying to get the collab from the server
  let err = test_client
    .api_client
    .get_collab(QueryCollabParams {
      object_id: object_id.clone(),
      collab_type: collab_type.clone(),
    })
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);

  // After reconnect the collab should be synced to the server.
  test_client.reconnect().await;
  // Wait for the messages to be sent
  tokio::time::sleep(Duration::from_secs(2)).await;

  assert_collab_json(
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    3,
    json!( {
      "0": "0",
      "1": "1",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
    }),
  )
  .await;
}