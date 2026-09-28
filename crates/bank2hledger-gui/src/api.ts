import { invoke } from "@tauri-apps/api/core";
import * as mock from "./mock";

// In dev builds without the Tauri IPC bridge (plain `npm run dev` in a
// browser), fall back to the synthetic mock in mock.ts so the UI can be
// developed and screenshotted without the Rust backend. Release builds
// never touch it: import.meta.env.DEV is statically false there, so the
// mock code is tree-shaken out entirely.
const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const useMock = import.meta.env.DEV && !inTauri;

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (useMock) {
    return mock.handle(cmd, args) as Promise<T>;
  }
  return invoke<T>(cmd, args);
}

export interface AccountInfo {
  name: string;
  profile: string;
  hledger_account: string;
  rules_file: string | null;
}

export interface WorkspaceInfo {
  config_path: string;
  journal: string;
  in_dir: string;
  staging_dir: string;
  rules_dir: string;
  accounts: AccountInfo[];
  hledger_version: string | null;
}

export interface InboxFile {
  file_name: string;
  path: string;
  size_bytes: number;
}

export interface AccountInbox {
  account: string;
  profile: string;
  files: InboxFile[];
  needs_pdftotext: boolean;
}

export interface PreviewTransaction {
  date: string;
  payee: string;
  amount: string;
  currency: string;
  external_id: string | null;
  notes: string | null;
}

export interface PreviewDto {
  account: string;
  new: PreviewTransaction[];
  already_seen: number;
}

export interface ImportResult {
  account: string;
  new_count: number;
  already_seen: number;
  preview: string | null;
}

export interface FileEntry {
  file_name: string;
  path: string;
}

export function loadWorkspace(path: string | null): Promise<WorkspaceInfo> {
  return call("load_workspace", { path });
}

export function initWorkspace(dir: string, force: boolean): Promise<WorkspaceInfo> {
  return call("init_workspace", { dir, force });
}

export function inboxFiles(): Promise<AccountInbox[]> {
  return call("inbox_files");
}

export function previewImport(account: string): Promise<PreviewDto> {
  return call("preview_import", { account });
}

export function runImport(account: string, dryRun: boolean): Promise<ImportResult> {
  return call("run_import", { account, dryRun });
}

export function getStatus(): Promise<string> {
  return call("get_status");
}

export function listRulesFiles(): Promise<FileEntry[]> {
  return call("list_rules_files");
}

export function openPath(path: string): Promise<void> {
  return call("open_path", { path });
}
