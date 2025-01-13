use candid::{Decode, Encode, Principal};
use ic_agent::Agent;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use worker::*;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TokenListItem {
    pub user_id: String,
    pub name: String,
    pub token_name: String,
    pub token_symbol: String,
    pub logo: String,
    pub description: String,
    pub created_at: String,
    pub formatted_created_at: String,
    pub link: String,
    #[serde(default)]
    pub is_nsfw: bool,
}

async fn fetch_documents(env: &Env) -> Result<Vec<TokenListItem>> {
    let firestore_url = format!(
        "https://firestore.googleapis.com/v1/projects/{project_id}/databases/(default)/documents/tokens",
        project_id = env.var("FIREBASE_PROJECT_ID")?.to_string(),
    );

    let client = reqwest::Client::new();
    let api_key = env.secret("FIREBASE_API_KEY")?.to_string();
    let response = client
        .get(firestore_url)
        .query(&[("key", &api_key)])
        .send()
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    if response.status().is_success() {
        let data: serde_json::Value = response.json().await
            .map_err(|e| Error::from(e.to_string()))?;
        
        let documents = data["documents"].as_array()
            .ok_or_else(|| Error::from("No documents found"))?;
        
        let firestore_entries: Vec<TokenListItem> = documents
            .iter()
            .filter_map(|doc| {
                serde_json::from_value(doc["fields"].clone()).ok()
            })
            .collect();
        
        Ok(firestore_entries)
    } else {
        Err(Error::from(format!(
            "Failed to fetch documents: {}",
            response.status()
        )))
    }
}

async fn create_agent(env: &Env) -> Result<Agent> {
    let pk = env.secret("RECLAIM_CANISTER_PEM")?.to_string();
    let identity = ic_agent::identity::BasicIdentity::from_pem(
        stringreader::StringReader::new(pk.as_str()),
    ).map_err(|e| Error::from(e.to_string()))?;

    let agent = Agent::builder()
        .with_url("https://a4gq6-oaaaa-aaaab-qaa4q-cai.raw.ic0.app")
        .with_identity(identity)
        .build()
        .map_err(|e| Error::from(e.to_string()))?;

    Ok(agent)
}

async fn get_user_canister(agent: &Agent, user_principal: Principal) -> Result<Option<Principal>> {
    let user_index = Principal::from_text("rimrc-piaaa-aaaao-aaljq-cai")
        .map_err(|e| Error::from(e.to_string()))?;

    let response = agent
        .query(&user_index, "get_user_canister_id_from_user_principal_id")
        .with_arg(Encode!(&user_principal).map_err(|e| Error::from(e.to_string()))?)
        .call()
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let canister: Option<Principal> = Decode!(&response, Option<Principal>)
        .map_err(|e| Error::from(e.to_string()))?;

    Ok(canister)
}

async fn find_invalid_tokens(env: &Env) -> Result<Vec<TokenListItem>> {
    let firestore_entries = fetch_documents(env).await?;
    
    let token_users: HashSet<String> = firestore_entries
        .iter()
        .map(|entry| entry.user_id.clone())
        .collect();
    
    let agent = create_agent(env).await?;
    let mut valid_canisters = HashSet::new();
    
    for user_id in token_users {
        if let Ok(user_principal) = Principal::from_text(&user_id) {
            if let Ok(Some(canister)) = get_user_canister(&agent, user_principal).await {
                valid_canisters.insert(canister);
            }
        }
    }
    
    let mut invalid_tokens = Vec::new();
    
    for entry in firestore_entries {
        let token_canister = entry.link
            .split('/')
            .last()
            .and_then(|id| Principal::from_text(id).ok());
            
        match token_canister {
            Some(principal) if !valid_canisters.contains(&principal) => {
                invalid_tokens.push(entry);
            }
            None => {
                invalid_tokens.push(entry);
            }
            _ => {}
        }
    }
    
    Ok(invalid_tokens)
}

async fn delete_invalid_tokens(
    env: &Env,
    invalid_tokens: &[TokenListItem],
) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let api_key = env.secret("FIREBASE_API_KEY")?.to_string();
    let project_id = env.var("FIREBASE_PROJECT_ID")?.to_string();
    let mut deleted_tokens = Vec::new();

    for token in invalid_tokens {
        let token_id = token.link
            .split('/')
            .last()
            .ok_or_else(|| Error::from("Invalid token link format"))?;

        let delete_url = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/tokens/{}",
            project_id,
            token_id
        );

        let response = client
            .delete(&delete_url)
            .query(&[("key", &api_key)])
            .send()
            .await
            .map_err(|e| Error::from(e.to_string()))?;

        if response.status().is_success() {
            deleted_tokens.push(token_id.to_string());
        }
    }

    Ok(deleted_tokens)
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/", |req, env, _ctx| async move {
            let cors = Cors::new()
                .with_origins(vec!["*"])
                .with_methods(vec![Method::Get, Method::Post, Method::Options])
                .with_max_age(86400);

            if req.method() == Method::Options {
                return Response::empty()
                    .map(|resp| cors.apply(resp));
            }

            let auth_header = req.headers()
                .get("Authorization")?
                .ok_or_else(|| Error::from("Missing authorization header"))?;

            if auth_header != env.secret("WORKER_AUTH_TOKEN")?.to_string() {
                return Response::error("Unauthorized", 401);
            }

            match find_invalid_tokens(&env).await {
                Ok(invalid_tokens) => {
                    let deletion_result = if !invalid_tokens.is_empty() {
                        delete_invalid_tokens(&env, &invalid_tokens).await?
                    } else {
                        Vec::new()
                    };

                    let json = serde_json::json!({
                        "invalid_tokens": invalid_tokens,
                        "count": invalid_tokens.len(),
                        "deleted_tokens": deletion_result,
                        "deleted_count": deletion_result.len()
                    });
                    
                    Response::from_json(&json)
                        .map(|resp| cors.apply(resp))
                }
                Err(e) => Response::error(format!("Error: {}", e), 500),
            }
        })
        .run(req, env)
        .await
}