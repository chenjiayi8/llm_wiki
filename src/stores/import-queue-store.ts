import { create } from "zustand"

export type ImportJobStatus =
  | "queued"
  | "copying"
  | "preprocessing"
  | "ingesting"
  | "retry_wait"
  | "completed"
  | "failed"

export interface ImportJobRecord {
  id: number
  batch_id: number
  source_path: string
  source_name: string
  dest_path: string
  status: ImportJobStatus
  attempt_count: number
}

export interface ImportQueueSummary {
  active_batches: number
  total_jobs: number
  queued_jobs: number
  running_jobs: number
  retrying_jobs: number
  completed_jobs: number
  failed_jobs: number
  is_idle: boolean
  headline: string
  max_concurrency: number
}

export function isImportQueueActive(summary: ImportQueueSummary | null): summary is ImportQueueSummary {
  return !!summary && !summary.is_idle
}

interface ImportQueueState {
  summary: ImportQueueSummary | null
  setSummary: (summary: ImportQueueSummary | null) => void
}

export const useImportQueueStore = create<ImportQueueState>((set) => ({
  summary: null,
  setSummary: (summary) => set({ summary }),
}))
