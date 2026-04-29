import { useState, useEffect, useCallback } from "react"
import { open } from "@tauri-apps/plugin-dialog"
import { Plus, FileText, RefreshCw, BookOpen, Trash2, FolderPlus } from "lucide-react"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { useWikiStore } from "@/stores/wiki-store"
import { listDirectory, readFile, writeFile, deleteFile, findRelatedWikiPages } from "@/commands/fs"
import { enqueueImportBatch } from "@/commands/import-queue"
import type { FileNode } from "@/types/wiki"
import { startIngest } from "@/lib/ingest"
import { useTranslation } from "react-i18next"
import { normalizePath, getFileName } from "@/lib/path-utils"

const IMPORT_FILE_FILTERS = [
  {
    name: "Documents",
    extensions: [
      "md", "mdx", "txt", "rtf", "pdf",
      "html", "htm", "xml",
      "doc", "docx", "xls", "xlsx", "ppt", "pptx",
      "odt", "ods", "odp", "epub", "pages", "numbers", "key",
    ],
  },
  {
    name: "Data",
    extensions: ["json", "jsonl", "csv", "tsv", "yaml", "yml", "ndjson"],
  },
  {
    name: "Code",
    extensions: [
      "py", "js", "ts", "jsx", "tsx", "rs", "go", "java",
      "c", "cpp", "h", "rb", "php", "swift", "sql", "sh",
    ],
  },
  {
    name: "Images",
    extensions: ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "tiff", "avif", "heic"],
  },
  {
    name: "Media",
    extensions: ["mp4", "webm", "mov", "avi", "mkv", "mp3", "wav", "ogg", "flac", "m4a"],
  },
  { name: "All Files", extensions: ["*"] },
]

const SUPPORTED_SOURCE_EXTENSIONS = new Set(
  IMPORT_FILE_FILTERS
    .flatMap((filter) => filter.extensions)
    .filter((extension) => extension !== "*")
    .map((extension) => extension.toLowerCase())
)

export function SourcesView() {
  const { t } = useTranslation()
  const project = useWikiStore((s) => s.project)
  const selectedFile = useWikiStore((s) => s.selectedFile)
  const setSelectedFile = useWikiStore((s) => s.setSelectedFile)
  const setActiveView = useWikiStore((s) => s.setActiveView)
  const setFileContent = useWikiStore((s) => s.setFileContent)
  const setFileTree = useWikiStore((s) => s.setFileTree)
  const setChatExpanded = useWikiStore((s) => s.setChatExpanded)
  const llmConfig = useWikiStore((s) => s.llmConfig)
  const [sources, setSources] = useState<FileNode[]>([])
  const [importing, setImporting] = useState(false)
  const [ingestingPath, setIngestingPath] = useState<string | null>(null)

  const loadSources = useCallback(async () => {
    if (!project) return
    const pp = normalizePath(project.path)
    try {
      const tree = await listDirectory(`${pp}/raw/sources`)
      const files = flattenFiles(tree)
      setSources(files)
    } catch {
      setSources([])
    }
  }, [project])

  useEffect(() => {
    loadSources()
  }, [loadSources])

  const queueImportedPaths = useCallback(async (paths: string[], rootPath: string) => {
    if (!project || paths.length === 0) return

    const normalizedPaths = dedupeNormalizedPaths(paths)
    if (normalizedPaths.length === 0) return

    const projectPath = normalizePath(project.path)
    await enqueueImportBatch(normalizePath(rootPath), normalizedPaths, projectPath)
    await loadSources()
    window.alert(t("sources.importQueued", { count: normalizedPaths.length }))
  }, [project, loadSources, t])

  async function handleImport() {
    if (!project) return

    const selected = await open({
      multiple: true,
      title: "Import Source Files",
      filters: IMPORT_FILE_FILTERS,
    })

    if (!selected || selected.length === 0) return

    setImporting(true)
    try {
      const paths = Array.isArray(selected) ? selected : [selected]
      const normalizedPaths = dedupeNormalizedPaths(paths)
      await queueImportedPaths(normalizedPaths, getParentDirectory(normalizedPaths[0]))
    } catch (err) {
      console.error("Failed to queue imported files:", err)
      window.alert(String(err))
    } finally {
      setImporting(false)
    }
  }

  async function handleImportFolder() {
    if (!project) return

    const selected = await open({
      directory: true,
      multiple: false,
      title: t("sources.importFolder"),
    })
    if (!selected || Array.isArray(selected)) return

    const rootPath = normalizePath(selected)
    setImporting(true)
    try {
      const sourcePaths = await scanSupportedFiles(rootPath)
      await queueImportedPaths(sourcePaths, rootPath)
    } catch (err) {
      console.error("Failed to queue imported folder:", err)
      window.alert(t("sources.importFolderFailed", { error: String(err) }))
    } finally {
      setImporting(false)
    }
  }

  async function handleOpenSource(node: FileNode) {
    setActiveView("wiki")
    setSelectedFile(node.path)
    try {
      const content = await readFile(node.path)
      setFileContent(content)
    } catch (err) {
      console.error("Failed to read source:", err)
    }
  }

  async function handleDelete(node: FileNode) {
    if (!project) return
    const pp = normalizePath(project.path)
    const fileName = node.name
    const confirmed = window.confirm(
      t("sources.deleteConfirm", { name: fileName })
    )
    if (!confirmed) return

    try {
      // Step 1: Find related wiki pages before deleting
      const relatedPages = await findRelatedWikiPages(pp, fileName)
      const deletedSlugs = relatedPages.map((p) => {
        const name = getFileName(p).replace(".md", "")
        return name
      }).filter(Boolean)

      // Step 2: Delete the source file
      await deleteFile(node.path)

      // Step 3: Delete preprocessed cache
      try {
        await deleteFile(`${pp}/raw/sources/.cache/${fileName}.txt`)
      } catch {
        // cache file may not exist
      }

      // Step 4: Delete or update related wiki pages
      // If a page has multiple sources, only remove this filename from sources[]; don't delete the page
      const actuallyDeleted: string[] = []
      for (const pagePath of relatedPages) {
        try {
          const content = await readFile(pagePath)
          // Parse sources from frontmatter
          const sourcesMatch = content.match(/^sources:\s*\[([^\]]*)\]/m)
          if (sourcesMatch) {
            const sourcesList = sourcesMatch[1]
              .split(",")
              .map((s) => s.trim().replace(/["']/g, ""))
              .filter((s) => s.length > 0)

            if (sourcesList.length > 1) {
              // Multiple sources — just remove this file from the list, keep the page
              const updatedSources = sourcesList.filter(
                (s) => s.toLowerCase() !== fileName.toLowerCase()
              )
              const updatedContent = content.replace(
                /^sources:\s*\[([^\]]*)\]/m,
                `sources: [${updatedSources.map((s) => `"${s}"`).join(", ")}]`
              )
              await writeFile(pagePath, updatedContent)
              continue // Don't delete this page
            }
          }

          // Single source or no sources field — delete the page
          await deleteFile(pagePath)
          actuallyDeleted.push(pagePath)
        } catch (err) {
          console.error(`Failed to process wiki page ${pagePath}:`, err)
        }
      }

      // Step 5: Clean index.md — remove entries for actually deleted pages only
      const deletedPageSlugs = actuallyDeleted.map((p) => {
        const name = getFileName(p).replace(".md", "")
        return name
      }).filter(Boolean)

      if (deletedPageSlugs.length > 0) {
        try {
          const indexPath = `${pp}/wiki/index.md`
          const indexContent = await readFile(indexPath)
          const updatedIndex = indexContent
            .split("\n")
            .filter((line) => !deletedPageSlugs.some((slug) => line.toLowerCase().includes(slug.toLowerCase())))
            .join("\n")
          await writeFile(indexPath, updatedIndex)
        } catch {
          // non-critical
        }
      }

      // Step 6: Clean [[wikilinks]] to deleted pages from remaining wiki files
      if (deletedPageSlugs.length > 0) {
        try {
          const wikiTree = await listDirectory(`${pp}/wiki`)
          const allMdFiles = flattenMdFiles(wikiTree)
          for (const file of allMdFiles) {
            try {
              const content = await readFile(file.path)
              let updated = content
              for (const slug of deletedPageSlugs) {
                const linkRegex = new RegExp(`\\[\\[${slug.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?:\\|([^\\]]+))?\\]\\]`, "gi")
                updated = updated.replace(linkRegex, (_match, displayText) => displayText || slug)
              }
              if (updated !== content) {
                await writeFile(file.path, updated)
              }
            } catch {
              // skip
            }
          }
        } catch {
          // non-critical
        }
      }

      // Step 7: Append deletion record to log.md
      try {
        const logPath = `${pp}/wiki/log.md`
        const logContent = await readFile(logPath).catch(() => "# Wiki Log\n")
        const date = new Date().toISOString().slice(0, 10)
        const keptCount = relatedPages.length - actuallyDeleted.length
        const logEntry = `\n## [${date}] delete | ${fileName}\n\nDeleted source file and ${actuallyDeleted.length} wiki pages.${keptCount > 0 ? ` ${keptCount} shared pages kept (have other sources).` : ""}\n`
        await writeFile(logPath, logContent.trimEnd() + logEntry)
      } catch {
        // non-critical
      }

      // Step 8: Refresh everything
      await loadSources()
      const tree = await listDirectory(pp)
      setFileTree(tree)
      useWikiStore.getState().bumpDataVersion()

      // Clear selected file if it was the deleted one
      if (selectedFile === node.path || actuallyDeleted.includes(selectedFile ?? "")) {
        setSelectedFile(null)
      }
    } catch (err) {
      console.error("Failed to delete source:", err)
      window.alert(`Failed to delete: ${err}`)
    }
  }

  async function handleIngest(node: FileNode) {
    if (!project || ingestingPath) return
    setIngestingPath(node.path)
    try {
      setChatExpanded(true)
      setActiveView("wiki")
      await startIngest(normalizePath(project.path), node.path, llmConfig)
    } catch (err) {
      console.error("Failed to start ingest:", err)
    } finally {
      setIngestingPath(null)
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b px-4 py-3">
        <h2 className="text-sm font-semibold">{t("sources.title")}</h2>
        <div className="flex gap-1">
          <Button variant="ghost" size="icon" onClick={loadSources} title="Refresh">
            <RefreshCw className="h-4 w-4" />
          </Button>
          <Button size="sm" onClick={handleImport} disabled={importing}>
            <Plus className="mr-1 h-4 w-4" />
            {importing ? t("sources.importing") : t("sources.import")}
          </Button>
          <Button variant="outline" size="sm" onClick={handleImportFolder} disabled={importing}>
            <FolderPlus className="mr-1 h-4 w-4" />
            {t("sources.importFolder")}
          </Button>
        </div>
      </div>

      <ScrollArea className="flex-1">
        {sources.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-3 p-8 text-center text-sm text-muted-foreground">
            <p>{t("sources.noSources")}</p>
            <p>{t("sources.importHint")}</p>
            <Button variant="outline" size="sm" onClick={handleImport}>
              <Plus className="mr-1 h-4 w-4" />
              {t("sources.importFiles")}
            </Button>
            <Button variant="outline" size="sm" onClick={handleImportFolder}>
              <FolderPlus className="mr-1 h-4 w-4" />
              {t("sources.importFolder")}
            </Button>
          </div>
        ) : (
          <div className="p-2">
            {sources.map((source) => (
              <div
                key={source.path}
                className="flex w-full items-center gap-1 rounded-md px-1 py-1 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
              >
                <button
                  onClick={() => handleOpenSource(source)}
                  className="flex flex-1 items-center gap-2 truncate px-2 py-1 text-left"
                >
                  <FileText className="h-4 w-4 shrink-0" />
                  <span className="truncate">{source.name}</span>
                </button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0"
                  title={t("sources.ingest")}
                  disabled={ingestingPath === source.path}
                  onClick={() => handleIngest(source)}
                >
                  <BookOpen className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
                  title={t("sources.delete")}
                  onClick={() => handleDelete(source)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
        )}
      </ScrollArea>

      <div className="border-t px-4 py-2 text-xs text-muted-foreground">
        {t("sources.sourceCount", { count: sources.length })}
      </div>
    </div>
  )
}

async function scanSupportedFiles(rootPath: string): Promise<string[]> {
  const queue = [normalizePath(rootPath)]
  const sourcePaths: string[] = []

  while (queue.length > 0) {
    const currentPath = queue.shift()
    if (!currentPath) continue

    const nodes = await listDirectory(currentPath)
    for (const node of nodes) {
      if (node.is_dir) {
        queue.push(normalizePath(node.path))
        continue
      }

      if (isSupportedSourceFile(node.path)) {
        sourcePaths.push(normalizePath(node.path))
      }
    }
  }

  return dedupeNormalizedPaths(sourcePaths)
}

function isSupportedSourceFile(path: string): boolean {
  const fileName = getFileName(path).toLowerCase()
  const extIndex = fileName.lastIndexOf(".")
  if (extIndex === -1 || extIndex === fileName.length - 1) return false
  const extension = fileName.slice(extIndex + 1)
  return SUPPORTED_SOURCE_EXTENSIONS.has(extension)
}

function dedupeNormalizedPaths(paths: string[]): string[] {
  return [...new Set(paths.map((path) => normalizePath(path)).filter(Boolean))]
}

function getParentDirectory(path?: string): string {
  if (!path) return ""
  const normalized = normalizePath(path).replace(/\/+$/, "")
  const lastSlashIndex = normalized.lastIndexOf("/")
  if (lastSlashIndex === -1) return normalized
  if (lastSlashIndex === 0) return "/"
  return normalized.slice(0, lastSlashIndex)
}

function flattenFiles(nodes: FileNode[]): FileNode[] {
  const files: FileNode[] = []
  for (const node of nodes) {
    if (node.is_dir && node.children) {
      files.push(...flattenFiles(node.children))
    } else if (!node.is_dir) {
      files.push(node)
    }
  }
  return files
}

function flattenMdFiles(nodes: FileNode[]): FileNode[] {
  const files: FileNode[] = []
  for (const node of nodes) {
    if (node.is_dir && node.children) {
      files.push(...flattenMdFiles(node.children))
    } else if (!node.is_dir && node.name.endsWith(".md")) {
      files.push(node)
    }
  }
  return files
}
