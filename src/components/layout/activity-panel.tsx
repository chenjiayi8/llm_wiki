import { useState, useEffect, useRef } from "react"
import {
  ChevronUp, ChevronDown, Loader2, CheckCircle2, AlertCircle,
  FileText, Users, Lightbulb, BookOpen, GitMerge, BarChart3, HelpCircle, Layout,
} from "lucide-react"
import { useActivityStore, type ActivityItem } from "@/stores/activity-store"
import { useImportQueueStore, isImportQueueActive } from "@/stores/import-queue-store"
import { useWikiStore } from "@/stores/wiki-store"
import { normalizePath, getFileName } from "@/lib/path-utils"

const FILE_TYPE_ICONS: Record<string, typeof FileText> = {
  sources: BookOpen,
  entities: Users,
  concepts: Lightbulb,
  queries: HelpCircle,
  synthesis: GitMerge,
  comparisons: BarChart3,
}

function getFileTypeInfo(path: string): { icon: typeof FileText; type: string } {
  for (const [dir, icon] of Object.entries(FILE_TYPE_ICONS)) {
    if (path.includes(`/${dir}/`) || path.startsWith(`wiki/${dir}/`)) {
      return { icon, type: dir.charAt(0).toUpperCase() + dir.slice(1, -1) }
    }
  }
  if (path.includes("index.md")) return { icon: Layout, type: "Index" }
  if (path.includes("log.md")) return { icon: FileText, type: "Log" }
  return { icon: FileText, type: "File" }
}

export function ActivityPanel() {
  const items = useActivityStore((s) => s.items)
  const clearDone = useActivityStore((s) => s.clearDone)
  const summary = useImportQueueStore((s) => s.summary)
  const [expanded, setExpanded] = useState(false)
  const prevRunningRef = useRef(0)

  const importActive = isImportQueueActive(summary)
  const runningCount = items.filter((i) => i.status === "running").length
  const hasItems = items.length > 0
  const showPanel = hasItems || importActive

  // Auto-expand when a new task starts running
  useEffect(() => {
    if (runningCount > 0 && prevRunningRef.current === 0) {
      setExpanded(true)
    }
    prevRunningRef.current = runningCount
  }, [runningCount])

  if (!showPanel) return null

  const latestItem = items[0]
  const headerText = importActive
    ? summary.headline
    : runningCount > 0
      ? `Processing: ${latestItem?.title ?? "..."}`
      : `Done: ${latestItem?.title ?? "All tasks complete"}`

  return (
    <div className="border-t bg-muted/30">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-xs text-muted-foreground hover:bg-accent/50"
      >
        {importActive || runningCount > 0 ? (
          <Loader2 className="h-3 w-3 animate-spin shrink-0" />
        ) : (
          <CheckCircle2 className="h-3 w-3 shrink-0 text-emerald-500" />
        )}
        <span className="flex-1 truncate text-left">
          {headerText}
        </span>
        {importActive && summary && (
          <span className="flex items-center gap-2 text-[10px]">
            <span className="rounded bg-background/70 px-1.5 py-0.5 tabular-nums">
              {summary.completed_jobs}/{summary.total_jobs}
            </span>
            <span className="rounded bg-background/70 px-1.5 py-0.5 tabular-nums">
              {summary.running_jobs} running
            </span>
          </span>
        )}
        {expanded ? (
          <ChevronDown className="h-3 w-3 shrink-0" />
        ) : (
          <ChevronUp className="h-3 w-3 shrink-0" />
        )}
      </button>

      {expanded && (
        <div className="max-h-64 overflow-y-auto border-t">
          {importActive && summary && (
            <div className="px-3 py-1.5 text-[10px] text-muted-foreground border-b border-border/50">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                <span className="tabular-nums">
                  Completed {summary.completed_jobs}/{summary.total_jobs}
                </span>
                <span className="tabular-nums">Running {summary.running_jobs}</span>
                <span className="tabular-nums">Max {summary.max_concurrency}</span>
                <span className="tabular-nums">Retrying {summary.retrying_jobs}</span>
                <span className="tabular-nums text-destructive">Failed {summary.failed_jobs}</span>
              </div>
            </div>
          )}
          {items.map((item) => (
            <ActivityRow key={item.id} item={item} />
          ))}
          {items.some((i) => i.status !== "running") && (
            <button
              onClick={clearDone}
              className="w-full px-3 py-1 text-center text-[10px] text-muted-foreground hover:underline"
            >
              Clear completed
            </button>
          )}
        </div>
      )}
    </div>
  )
}

function ActivityRow({ item }: { item: ActivityItem }) {
  const setSelectedFile = useWikiStore((s) => s.setSelectedFile)
  const project = useWikiStore((s) => s.project)

  function handleFileClick(filePath: string) {
    if (!project) return
    const pp = normalizePath(project.path)
    const fullPath = filePath.startsWith("/") ? normalizePath(filePath) : `${pp}/${filePath}`
    setSelectedFile(fullPath)
  }

  return (
    <div className="px-3 py-2 text-xs border-b border-border/50 last:border-b-0">
      <div className="flex items-start gap-2">
        <div className="mt-0.5 shrink-0">
          {item.status === "running" && <Loader2 className="h-3 w-3 animate-spin text-primary" />}
          {item.status === "done" && <CheckCircle2 className="h-3 w-3 text-emerald-500" />}
          {item.status === "error" && <AlertCircle className="h-3 w-3 text-destructive" />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="font-medium">{item.title}</div>
          <div className="text-muted-foreground mt-0.5">{item.detail}</div>
        </div>
      </div>

      {/* File list with types */}
      {item.filesWritten.length > 0 && item.status === "done" && (
        <div className="mt-1.5 ml-5 flex flex-col gap-0.5">
          {item.filesWritten.map((filePath) => {
            const { icon: Icon, type } = getFileTypeInfo(filePath)
            const fileName = getFileName(filePath)
            return (
              <button
                key={filePath}
                type="button"
                onClick={() => handleFileClick(filePath)}
                className="flex items-center gap-1.5 rounded px-1 py-0.5 text-left text-muted-foreground hover:bg-accent/50 hover:text-foreground transition-colors"
              >
                <Icon className="h-3 w-3 shrink-0" />
                <span className="text-[10px] font-medium text-muted-foreground/70 w-14 shrink-0">{type}</span>
                <span className="truncate">{fileName}</span>
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}
