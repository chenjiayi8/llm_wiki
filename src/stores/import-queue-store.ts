import { create } from "zustand"

export interface ImportJobRecord {
  id: number
  root_path: string
  source_path: string
  project_path: string
  status: string
  attempt_count: number
  last_error: string | null
  files_written_json: string | null
  created_at: string
  updated_at: string
  claimed_at: string | null
}

export interface ImportQueueSummary {
  pending: number
  processing: number
  completed: number
  failed: number
  total: number
  jobs: ImportJobRecord[]
}

interface ImportQueueState {
  summary: ImportQueueSummary | null
  setSummary: (summary: ImportQueueSummary | null) => void
}

export const useImportQueueStore = create<ImportQueueState>((set) => ({
  summary: null,
  setSummary: (summary) => set({ summary }),
}))
