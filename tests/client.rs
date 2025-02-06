use std::{env, time::Duration};
use twilio::{Client, OutboundMessage};

#[tokio::test]
async fn send_sms() {
    dotenv::dotenv().ok();

    let account_id = env::var("ACCOUNT_ID").expect("Find ACCOUNT_ID environment variable");
    let auth_token = env::var("AUTH_TOKEN").expect("Find AUTH_TOKEN environment variable");
    let from = env::var("FROM").expect("Find FROM environment variable");
    let to = env::var("TO").expect("Find TO environment variable");

    let client = Client::new(&account_id, &auth_token);
    let msg_sid = client
        .send_message(OutboundMessage::new(&from, &to, "Hello, World!"))
        .await
        .expect("to send message")
        .sid;

    tokio::time::sleep(Duration::from_secs(7)).await;

    let status = client
        .get_message_status(&msg_sid)
        .await
        .expect("getting message status")
        .status
        .expect("Didn't get a status back");

    println!("STATUS: {:?}", &status);

    assert!(matches!(status, twilio::MessageStatus::sent));
}
