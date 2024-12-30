use std::collections::HashMap;
use worker::*;
use serde_json::json;
use reqwest::Client;
use gcloud_sdk::google::cloud::bigquery::v2::Job;
use gcloud_sdk::*;
use std::env;


#[event(fetch)]
pub async fn main(req: Request, env: Env, ctx: worker::Context) -> Result<Response> {
    match init(&env).await {
        Ok(message) => Response::ok(message),
        Err(e) => Response::error(e.to_string(), 500),
    }
    match run_reconciliation(&env).await {
        Ok(_) => Response::ok("Reconciliation completed."),
        Err(e) => Response::error(e.to_string(), 500),
    }

    let keys = list_all_entries(&env).await?;
    Response::ok(format!("Keys: {:?}", keys));

    match remove_entries(keys).await {
        Ok(_) => Response::ok("Entries removed."),
        Err(e) => Response::error(e.to_string(), 500),
    }
}

async fn init(env: &Env) -> Result<String, Box<dyn std::error::Error>> {
    // Step 1: Set up a Cloudflare KV database
    setup_kv_database(env).await?;

    // Step 2: Snapshot Firestore Database
    let firestore_snapshot = snapshot_firestore(env).await?;

    // Step 3: Snapshot BigQuery Dataset
    let bigquery_snapshot = snapshot_bigquery(env).await?;

    // Step 4: Check that everything worked out fine
    if !firestore_snapshot.success || !bigquery_snapshot.success {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Snapshot failed: Firestore: {:?}, BigQuery: {:?}",
                firestore_snapshot, bigquery_snapshot
            ),
        )));
    }

    Ok("Snapshots completed and initiated.".to_string())
}

async fn remove_entries(keys: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    // fetch kv date from cloudflarekv with keys
    // Remove all entries from firebase, bigquery and cloudflare kv
   
    // delete_token_from_firestore(token_id).await?;
    // delete_rows_from_bigquery(dataset, table, condition).await?;

    // let kv = env.kv("MY_KV_NAMESPACE")?;
    // kv.delete("actionable_item")?.execute().await?;
    // Ok("Data removed".to_string());

    Ok(())
}

pub async fn delete_token_from_firestore(token_id: &str) -> Result<(), FirestoreError> {
    // Initialize Firestore
    let firestore_db: FirestoreDb = expect_context(); // Ensure Firestore context is available

    // Define the collection name
    const TEST_COLLECTION_NAME: &str = "tokens-list";

    // Delete the document
    firestore_db
        .fluent()
        .delete()
        .from(TEST_COLLECTION_NAME)
        .document_id(token_id)
        .execute()
        .await?;

    Ok(())
}

pub async fn delete_rows_from_bigquery(
    dataset_name: &str,
    table_name: &str,
    condition: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the Google Cloud client
    let google_project_id = env::var("GOOGLE_PROJECT_ID")?; // Replace with your project ID or load from the environment
    let bigquery_client = gcloud_sdk::GoogleCloudClient::new_default().await?;

    // Build the DELETE SQL query
    let delete_query = format!(
        "DELETE FROM `{project_id}.{dataset}.{table}` WHERE {condition}",
        project_id = google_project_id,
        dataset = dataset_name,
        table = table_name,
        condition = condition
    );

    println!("Executing query: {}", delete_query);

    // Execute the DELETE query
    let job = Job {
        configuration: Some(gcloud_sdk::google::cloud::bigquery::v2::JobConfiguration {
            query: Some(gcloud_sdk::google::cloud::bigquery::v2::JobConfigurationQuery {
                query: Some(delete_query),
                use_legacy_sql: Some(false), // Use Standard SQL
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let response = bigquery_client
        .bigquery()
        .jobs()
        .insert(job, &google_project_id)
        .await?;

    println!("Delete job submitted: {:?}", response);

    Ok(())
}


#[derive(Debug)]
struct SnapshotResult {
    success: bool,
    data: Option<String>,
    error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct Actionable {
    info: String,
    action: String,
    location: String,
}

async fn setup_kv_database(env: &Env) -> Result<(), Box<dyn std::error::Error>> {
    let kv = env.kv("MY_KV_NAMESPACE")?;

    // Initialize some example data
    kv.put("initialized", "true")?.execute().await?;
    kv.put("last_updated", chrono::Utc::now().to_string().as_str())?.execute().await?;

    console_log!("KV database setup completed.");

    Ok(())
}

async fn snapshot_firestore(env: &Env) -> Result<SnapshotResult, Box<dyn std::error::Error>> {
    let firestore_url = "https://firestore.googleapis.com/v1/projects/YOUR_PROJECT_ID/databases/(default)/documents:export";
    let token = get_access_token(env).await?;

    let client = Client::new();
    let response = client
        .post(firestore_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(SnapshotResult {
            success: true,
            data: Some(response.text().await?),
            error: None,
        })
    } else {
        Ok(SnapshotResult {
            success: false,
            data: None,
            error: Some(response.text().await?),
        })
    }
}

async fn snapshot_bigquery(env: &Env) -> Result<SnapshotResult, Box<dyn std::error::Error>> {
    let bigquery_url = "https://bigquery.googleapis.com/bigquery/v2/projects/YOUR_PROJECT_ID/jobs";
    let token = get_access_token(env).await?;

    let body = json!({
        "configuration": {
            "extract": {
                "sourceTable": {
                    "projectId": "YOUR_PROJECT_ID",
                    "datasetId": "YOUR_DATASET_ID",
                    "tableId": "YOUR_TABLE_ID"
                },
                "destinationUris": ["gs://YOUR_BUCKET_NAME/your-snapshot-file"],
                "destinationFormat": "NEWLINE_DELIMITED_JSON"
            }
        }
    });

    let client = Client::new();
    let response = client
        .post(bigquery_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await?;

    if response.status().is_success() {
        Ok(SnapshotResult {
            success: true,
            data: Some(response.text().await?),
            error: None,
        })
    } else {
        Ok(SnapshotResult {
            success: false,
            data: None,
            error: Some(response.text().await?),
        })
    }
}

async fn get_access_token(env: &Env) -> Result<String, Box<dyn std::error::Error>> {
    let client_email = env.var("GCP_CLIENT_EMAIL")?.to_string();
    let private_key = env.var("GCP_PRIVATE_KEY")?.to_string();

    let header = base64::encode(serde_json::to_string(&json!({
        "alg": "RS256",
        "typ": "JWT"
    }))?);

    let iat = chrono::Utc::now().timestamp();
    let exp = iat + 3600;
    let payload = base64::encode(serde_json::to_string(&json!({
        "iss": client_email,
        "scope": "https://www.googleapis.com/auth/cloud-platform",
        "aud": "https://oauth2.googleapis.com/token",
        "exp": exp,
        "iat": iat
    }))?);

    let unsigned_token = format!("{}.{}", header, payload);

    let key = rsa::RsaPrivateKey::from_pkcs1_pem(&private_key)?;
    let signature = base64::encode(key.sign(
        rsa::PaddingScheme::PKCS1v15Sign { hash: None },
        &sha2::Sha256::digest(unsigned_token.as_bytes())
    )?);

    let jwt = format!("{}.{}", unsigned_token, signature);

    let client = Client::new();
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion={}", jwt))
        .send()
        .await?;

    let json: HashMap<String, String> = response.json().await?;
    Ok(json.get("access_token").ok_or("Missing access token")?.to_string())
}

async fn run_reconciliation(env: &Env) -> Result<(), Box<dyn std::error::Error>> {
    
    // Reconciliation logic

    let actionable = Actionable {
        info: "hash_value_example".to_string(),
        action: "sync".to_string(),
        location: "us-central1".to_string(),
    };

    let kv = env.kv("MY_KV_NAMESPACE")?;
    kv.put("actionable_item", &serde_json::to_string(&actionable)?)?.execute().await?;

    console_log!("Reconciliation logic executed with actionable item: {:?}", actionable);

    Ok(())
}

async fn list_all_entries(env: &Env) -> Result<Vec<String>> {
    let kv = env.kv("MY_KV_NAMESPACE")?;
    let mut cursor: Option<String> = None;
    let mut all_keys = Vec::new();

    loop {
        let list_result = kv.list().cursor(cursor).execute().await?;
        all_keys.extend(list_result.keys.into_iter().map(|key| key.name));
        if list_result.cursor.is_none() {
            break;
        }
        cursor = list_result.cursor;
    }
    Ok(all_keys)
}
