import {
  claimImportJobs,
  completeImportJob,
  failImportJob,
  getImportQueueSummary,
  updateImportJobStage,
} from "@/commands/import-queue"
import { copyFile, preprocessFile } from "@/commands/fs"
import { autoIngest } from "@/lib/ingest"
import { useWikiStore } from "@/stores/wiki-store"
import type { ImportJobRecord } from "@/stores/import-queue-store"

const LOOP_INTERVAL_MS = 1000

let runnerStarted = false

export function startImportQueueRunner() {
  if (runnerStarted) return
  runnerStarted = true
  void runLoop()
}

async function runLoop() {
  try {
    const summary = await getImportQueueSummary()
    const availableSlots = Math.max(0, summary.max_concurrency - summary.running_jobs)

    if (availableSlots > 0) {
      const jobs = await claimImportJobs(availableSlots)
      await Promise.all(jobs.map((job) => runJob(job)))
    }
  } catch (error) {
    console.error("Import queue runner loop failed:", error)
  } finally {
    window.setTimeout(() => {
      void runLoop()
    }, LOOP_INTERVAL_MS)
  }
}

async function runJob(job: ImportJobRecord) {
  const { project, llmConfig } = useWikiStore.getState()
  if (!project) {
    return
  }

  try {
    await updateImportJobStage(job.id, "copying")
    await copyFile(job.source_path, job.dest_path)

    await updateImportJobStage(job.id, "preprocessing")
    await preprocessFile(job.dest_path)

    await updateImportJobStage(job.id, "ingesting")
    const filesWritten = await autoIngest(project.path, job.dest_path, llmConfig)

    await completeImportJob(job.id, JSON.stringify(filesWritten))
  } catch (error) {
    await failImportJob(job.id, job.attempt_count + 1, String(error))
  }
}
