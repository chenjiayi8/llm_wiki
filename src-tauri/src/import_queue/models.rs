use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportJobStatus {
    Queued,
    Copying,
    Preprocessing,
    Ingesting,
    RetryWait,
    Completed,
    Failed,
}

impl ImportJobStatus {
    pub fn from_wire(value: &str) -> Result<Self, String> {
        match value {
            "queued" => Ok(Self::Queued),
            "copying" => Ok(Self::Copying),
            "preprocessing" => Ok(Self::Preprocessing),
            "ingesting" => Ok(Self::Ingesting),
            "retry_wait" => Ok(Self::RetryWait),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            other => Err(format!("Unknown import job status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStage {
    Copying,
    Preprocessing,
    Ingesting,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportJobRecord {
    pub id: i64,
    pub batch_id: i64,
    pub source_path: String,
    pub source_name: String,
    pub dest_path: String,
    pub status: ImportJobStatus,
    pub attempt_count: i64,
}

#[derive(Debug, Clone)]
pub struct NewImportJob {
    pub batch_id: i64,
    pub source_path: String,
    pub source_name: String,
    pub dest_path: String,
    pub max_attempts: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportBatchSummary {
    pub active_batches: i64,
    pub total_jobs: i64,
    pub queued_jobs: i64,
    pub running_jobs: i64,
    pub retrying_jobs: i64,
    pub completed_jobs: i64,
    pub failed_jobs: i64,
    pub is_idle: bool,
    pub headline: String,
    pub max_concurrency: usize,
}
