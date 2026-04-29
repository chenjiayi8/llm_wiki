import { invoke } from "@tauri-apps/api/core"
import type {
  ImportJobRecord,
  ImportJobStatus,
  ImportQueueSummary,
} from "@/stores/import-queue-store"
import { normalizePath } from "@/lib/path-utils"

export async function enqueueImportBatch(
  rootPath: string,
  sourcePaths: string[],
  projectPath: string
): Promise<number> {
  const normalizedRootPath = normalizePath(rootPath)
  const normalizedProjectPath = normalizePath(projectPath)
  const normalizedSourcePaths = [
    ...new Set(sourcePaths.map((path) => normalizePath(path)).filter(Boolean)),
  ]

  return invoke<number>("enqueue_import_batch", {
    rootPath: normalizedRootPath,
    sourcePaths: normalizedSourcePaths,
    projectPath: normalizedProjectPath,
  })
}

export async function getImportQueueSummary(): Promise<ImportQueueSummary> {
  return invoke<ImportQueueSummary>("get_import_queue_summary")
}

export async function claimImportJobs(limit: number): Promise<ImportJobRecord[]> {
  return invoke<ImportJobRecord[]>("claim_import_jobs", { limit })
}

export async function updateImportJobStage(
  jobId: number,
  status: ImportJobStatus
): Promise<void> {
  return invoke<void>("update_import_job_stage", { jobId, status })
}

export async function completeImportJob(
  jobId: number,
  filesWrittenJson: string
): Promise<void> {
  return invoke<void>("complete_import_job", { jobId, filesWrittenJson })
}

export async function failImportJob(
  jobId: number,
  attemptCount: number,
  lastError: string
): Promise<void> {
  return invoke<void>("fail_import_job", { jobId, attemptCount, lastError })
}
