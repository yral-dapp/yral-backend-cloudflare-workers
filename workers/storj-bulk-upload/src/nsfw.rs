use std::{collections::BTreeMap, fmt::Display};

use anyhow::Context;
use reqwest::Url;

pub struct NsfwResolver {}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum IsNsfw {
    Yes,
    No,
    Maybe,
}

impl Display for IsNsfw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            IsNsfw::Yes => "yes",
            IsNsfw::No => "no",
            IsNsfw::Maybe => "maybe",
        };

        write!(f, "{text}")
    }
}

async fn query_nsfw_probability(ids: &[String]) -> anyhow::Result<BTreeMap<String, f64>> {
    let path: Url = "https://pr-169-yral-dapp-off-chain-agent.fly.dev/__private/nsfw-probability"
        .parse()
        .expect("URL to be valid");

    let client = reqwest::Client::new();
    let res: Vec<(String, f64)> = client
        .post(path)
        .json(ids)
        .send()
        .await
        .context("failed to send nsfw query")?
        .json()
        .await
        .context("failed to decode response for nsfw query")?;

    Ok(res.into_iter().collect())
}

impl NsfwResolver {
    pub async fn is_nsfw(ids: &[String]) -> anyhow::Result<Vec<(String, IsNsfw)>> {
        let probs = query_nsfw_probability(ids).await?;

        let res = ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    probs
                        .get(id)
                        .map(|&prob| if prob >= 0.4 { IsNsfw::Yes } else { IsNsfw::No })
                        .unwrap_or(IsNsfw::Maybe),
                )
            })
            .collect();

        Ok(res)
    }
}
