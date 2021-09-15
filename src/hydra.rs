use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct Job {
    pub buildoutputs: HashMap<String, BuildOutput>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BuildOutput {
    pub path: String,
}

pub async fn get_latest_job(
    server: &str,
    project: &str,
    jobset: &str,
    job: &str,
) -> Result<Job, reqwest::Error> {
    reqwest::Client::new()
        .get(format!(
            "https://{}/job/{}/{}/{}/latest",
            server, project, jobset, job
        ))
        .header("Accept", "application/json")
        .send()
        .await?
        .json::<Job>()
        .await
}
