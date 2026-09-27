import { invoke } from "@tauri-apps/api/core";

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
  return invoke("load_workspace", { path });
}

export function initWorkspace(dir: string, force: boolean): Promise<WorkspaceInfo> {
  return invoke("init_workspace", { dir, force });
}

export function inboxFiles(): Promise<AccountInbox[]> {
  return invoke("inbox_files");
}

export function previewImport(account: string): Promise<PreviewDto> {
  return invoke("preview_import", { account });
}

export function runImport(account: string, dryRun: boolean): Promise<ImportResult> {
  return invoke("run_import", { account, dryRun });
}

export function getStatus(): Promise<string> {
  return invoke("get_status");
}

export function listRulesFiles(): Promise<FileEntry[]> {
  return invoke("list_rules_files");
}

export function openPath(path: string): Promise<void> {
  return invoke("open_path", { path });
}
