// Mock backend so the UI can run standalone in a plain browser (`npm run dev`
// then open http://localhost:5173) without the Tauri shell. Active only when
// the Tauri IPC bridge is absent. Data is synthetic.

import {
  AccountInbox,
  DuplicateRow,
  FileEntry,
  ImportResult,
  PreviewDto,
  WorkspaceInfo,
} from "./api";

const workspace: WorkspaceInfo = {
  config_path: "C:\\Users\\Afonso\\Finance\\bank2hledger.toml",
  journal: "C:\\Users\\Afonso\\Finance\\2026.journal",
  in_dir: "C:\\Users\\Afonso\\Finance\\in",
  staging_dir: "C:\\Users\\Afonso\\Finance\\staging",
  rules_dir: "C:\\Users\\Afonso\\Finance\\rules",
  accounts: [
    {
      name: "monzo-personal",
      profile: "monzo_csv",
      hledger_account: "assets:banks:monzo:personal",
      rules_file: "C:\\Users\\Afonso\\Finance\\rules\\monzo-personal.rules",
    },
    {
      name: "revolut-personal",
      profile: "revolut_xls",
      hledger_account: "assets:banks:revolut:personal",
      rules_file: "C:\\Users\\Afonso\\Finance\\rules\\revolut-personal.rules",
    },
    {
      name: "wise-personal",
      profile: "wise_csv",
      hledger_account: "assets:banks:wise:personal",
      rules_file: null,
    },
  ],
  hledger_version: "hledger 1.42.1-g0bd3840b-20260621, windows-x86_64",
};

const inbox: AccountInbox[] = [
  {
    account: "monzo-personal",
    profile: "monzo_csv",
    needs_pdftotext: false,
    files: [
      { file_name: "monzo-personal.csv", path: "…\\in\\monzo-personal.csv", size_bytes: 24_876 },
      { file_name: "monzo-personal-2026-09-28.csv", path: "…\\in\\monzo-personal-2026-09-28.csv", size_bytes: 3_104 },
    ],
  },
  {
    account: "revolut-personal",
    profile: "revolut_xls",
    needs_pdftotext: false,
    files: [
      { file_name: "revolut-personal-2026-09.xls", path: "…\\in\\revolut-personal-2026-09.xls", size_bytes: 18_212 },
    ],
  },
  {
    account: "wise-personal",
    profile: "wise_csv",
    needs_pdftotext: false,
    files: [],
  },
];

/// Duplicate candidates for the mock workspace, keyed by account. The
/// staged keys match the preview rows' external ids (dedup-key format).
const duplicates: Record<string, DuplicateRow[]> = {
  "monzo-personal": [
    {
      staged_key: "id:tx_09Kd2",
      date: "2026-09-21",
      payee: "Corner Grocer",
      amount: "-23.14",
      currency: "GBP",
      candidates: [
        {
          date: "2026-09-14",
          payee: "Corner Grocer",
          amount: "-23.14",
          currency: "GBP",
          score: 0.92,
        },
      ],
    },
    {
      staged_key: "id:tx_09Kh7",
      date: "2026-09-24",
      payee: "Netflix",
      amount: "-10.99",
      currency: "GBP",
      candidates: [
        {
          date: "2026-09-24",
          payee: "Netflix subscription",
          amount: "-10.99",
          currency: "GBP",
          score: 1.0,
        },
      ],
    },
  ],
};

const preview: Record<string, PreviewDto> = {
  "monzo-personal": {
    account: "monzo-personal",
    already_seen: 14,
    duplicates: [],
    new: [
      { date: "2026-09-21", payee: "Corner Grocer", amount: "-23.14", currency: "GBP", external_id: "tx_09Kd2", notes: null },
      { date: "2026-09-22", payee: "Transport for London", amount: "-5.60", currency: "GBP", external_id: "tx_09Ke9", notes: null },
      { date: "2026-09-22", payee: "Uber", amount: "-12.40", currency: "GBP", external_id: "tx_09Kf1", notes: null },
      { date: "2026-09-23", payee: "PureGym", amount: "-44.99", currency: "GBP", external_id: "tx_09Kg3", notes: null },
      { date: "2026-09-24", payee: "Netflix", amount: "-10.99", currency: "GBP", external_id: "tx_09Kh7", notes: null },
      { date: "2026-09-25", payee: "Acme Ltd — salary", amount: "2450.00", currency: "GBP", external_id: "tx_09Kj2", notes: null },
      { date: "2026-09-26", payee: "Tesco Metro", amount: "-31.87", currency: "GBP", external_id: "tx_09Kk5", notes: null },
      { date: "2026-09-27", payee: "Caffè Nero", amount: "-3.80", currency: "GBP", external_id: "tx_09Km8", notes: null },
      { date: "2026-09-27", payee: "Caffè Nero", amount: "-3.80", currency: "GBP", external_id: "tx_09Km9", notes: null },
    ],
  },
};

const rules: FileEntry[] = [
  { file_name: "monzo-personal.rules", path: "…\\rules\\monzo-personal.rules" },
  { file_name: "revolut-personal.rules", path: "…\\rules\\revolut-personal.rules" },
];

const status = `               -23.14 GBP  assets:banks:monzo:personal
                 0.00 GBP  assets:banks:revolut:personal
                 0.00 GBP  assets:banks:wise:personal
                -23.14 GBP
`;

export function handle(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const ok = (v: unknown) => Promise.resolve(v);
  switch (cmd) {
    case "load_workspace":
    case "init_workspace":
      return ok(workspace);
    case "inbox_files":
      return ok(inbox);
    case "preview_import": {
      const p = preview[args.account as string] ?? {
        account: args.account as string,
        already_seen: 0,
        duplicates: [],
        new: [],
      };
      return args.resolveDuplicates
        ? ok({ ...p, duplicates: duplicates[args.account as string] ?? [] })
        : ok({ ...p, duplicates: [] });
    }
    case "run_import": {
      const p = preview[args.account as string];
      const skipKeys = (args.skipKeys as string[] | undefined) ?? [];
      const result: ImportResult = {
        account: args.account as string,
        new_count: Math.max(0, (p?.new.length ?? 0) - skipKeys.length),
        already_seen: p?.already_seen ?? 0,
        skipped_as_duplicates: skipKeys.length,
        preview: null,
      };
      return ok(result);
    }
    case "get_status":
      return ok(status);
    case "list_rules_files":
      return ok(rules);
    case "open_path":
      return ok(undefined);
    default:
      return Promise.reject(new Error(`no mock for command ${cmd}`));
  }
}
