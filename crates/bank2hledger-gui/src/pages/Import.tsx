import React from "react";
import {
  Button,
  Card,
  createTableColumn,
  DataGrid,
  DataGridBody,
  DataGridCell,
  DataGridHeader,
  DataGridHeaderCell,
  DataGridRow,
  makeStyles,
  MessageBar,
  MessageBarBody,
  shorthands,
  Spinner,
  TableColumnDefinition,
  Text,
  Title2,
  Title3,
  tokens,
} from "@fluentui/react-components";
import { inboxFiles, previewImport, runImport, AccountInbox, PreviewDto, PreviewTransaction, WorkspaceInfo } from "../api";

const useStyles = makeStyles({
  root: { display: "flex", flexDirection: "column", ...shorthands.gap("12px") },
  card: { ...shorthands.padding("16px"), display: "flex", flexDirection: "column", ...shorthands.gap("8px") },
  tableWrap: { maxHeight: "320px", overflowY: "auto", ...shorthands.borderRadius("6px") },
  fileRow: { display: "flex", justifyContent: "space-between", ...shorthands.gap("12px") },
  amountIn: { color: tokens.colorPaletteGreenForeground1 },
  amountOut: { color: tokens.colorPaletteRedForeground1 },
  actions: { display: "flex", ...shorthands.gap("8px") },
});

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

const columns: TableColumnDefinition<PreviewTransaction>[] = [
  createTableColumn<PreviewTransaction>({
    columnId: "date",
    compare: (a, b) => a.date.localeCompare(b.date),
    renderHeaderCell: () => "Date",
    renderCell: (item) => item.date,
  }),
  createTableColumn<PreviewTransaction>({
    columnId: "payee",
    compare: (a, b) => a.payee.localeCompare(b.payee),
    renderHeaderCell: () => "Description",
    renderCell: (item) => item.payee,
  }),
  createTableColumn<PreviewTransaction>({
    columnId: "amount",
    compare: (a, b) => Number(a.amount) - Number(b.amount),
    renderHeaderCell: () => "Amount",
    renderCell: (item) => (
      <span
        style={{ color: Number(item.amount) < 0 ? tokens.colorPaletteRedForeground1 : tokens.colorPaletteGreenForeground1 }}
      >
        {item.amount}
      </span>
    ),
  }),
  createTableColumn<PreviewTransaction>({
    columnId: "currency",
    compare: (a, b) => a.currency.localeCompare(b.currency),
    renderHeaderCell: () => "Currency",
    renderCell: (item) => item.currency,
  }),
  createTableColumn<PreviewTransaction>({
    columnId: "id",
    compare: (a, b) => (a.external_id ?? "").localeCompare(b.external_id ?? ""),
    renderHeaderCell: () => "Bank ID",
    renderCell: (item) => item.external_id ?? "—",
  }),
];

type AccountState = {
  preview: PreviewDto | null;
  busy: boolean;
  importing: boolean;
  result: string | null;
  error: string | null;
};

export function ImportPage({ workspace }: { workspace: WorkspaceInfo }) {
  const styles = useStyles();
  const [accounts, setAccounts] = React.useState<AccountInbox[] | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [states, setStates] = React.useState<Record<string, AccountState>>({});

  const refresh = React.useCallback(async () => {
    setError(null);
    try {
      setAccounts(await inboxFiles());
      setStates({});
    } catch (e) {
      setError(String(e));
    }
  }, []);

  React.useEffect(() => {
    void refresh();
  }, [refresh, workspace.config_path]);

  const setState = (account: string, patch: Partial<AccountState>) =>
    setStates((prev) => {
      const base: AccountState = prev[account] ?? {
        preview: null,
        busy: false,
        importing: false,
        result: null,
        error: null,
      };
      return { ...prev, [account]: { ...base, ...patch } };
    });

  const doPreview = async (account: string) => {
    setState(account, { busy: true, result: null, error: null });
    try {
      setState(account, { preview: await previewImport(account), busy: false });
    } catch (e) {
      setState(account, { busy: false, error: String(e) });
    }
  };

  const doImport = async (account: string, dryRun: boolean) => {
    setState(account, { importing: true, error: null });
    try {
      const r = await runImport(account, dryRun);
      setState(account, {
        importing: false,
        result:
          r.new_count === 0
            ? "Nothing new to import."
            : `Imported ${r.new_count} transaction(s) (${r.already_seen} already imported). Now check Balances against your bank app, then git commit to approve.`,
      });
    } catch (e) {
      setState(account, { importing: false, error: String(e) });
    }
  };

  if (error) {
    return (
      <>
        <Title2>Import</Title2>
        <MessageBar intent="error">
          <MessageBarBody>{error}</MessageBarBody>
        </MessageBar>
      </>
    );
  }
  if (!accounts) return <Spinner label="Reading inbox…" />;

  return (
    <div className={styles.root}>
      <Title2>Import</Title2>
      <Text>
        Files bind to accounts by filename prefix (<code>&lt;account&gt;*.csv</code>) — there
        is no guessing. Preview first; nothing is written until you import.
      </Text>
      {accounts.map((a) => {
        const s = states[a.account] ?? {
          preview: null, busy: false, importing: false, result: null, error: null,
        };
        return (
          <Card key={a.account} className={styles.card}>
            <Title3>{a.account}</Title3>
            <div className={styles.fileRow}>
              <Text>Profile: <b>{a.profile}</b></Text>
              <Text>
                {a.files.length === 0
                  ? "No matching files in the inbox."
                  : `${a.files.length} file(s): ${a.files.map((f) => `${f.file_name} (${formatBytes(f.size_bytes)})`).join(", ")}`}
              </Text>
            </div>
            {a.needs_pdftotext && (
              <MessageBar intent="warning">
                <MessageBarBody>
                  The <b>aqua_pdf</b> profile shells out to <code>pdftotext</code> (Poppler),
                  which is not bundled with this app. Install Poppler for Windows and put it
                  on PATH to use this profile.
                </MessageBarBody>
              </MessageBar>
            )}
            {s.error && (
              <MessageBar intent="error">
                <MessageBarBody>{s.error}</MessageBarBody>
              </MessageBar>
            )}
            {s.result && (
              <MessageBar intent="success">
                <MessageBarBody>{s.result}</MessageBarBody>
              </MessageBar>
            )}
            {s.busy && <Spinner size="tiny" label="Parsing…" />}

            {s.preview && (
              <>
                <Text>
                  {s.preview.new.length} new transaction(s); {s.preview.already_seen} already
                  imported (deduplicated). Review, then import.
                </Text>
                {s.preview.new.length > 0 && (
                  <div className={styles.tableWrap}>
                    <DataGrid items={s.preview.new} columns={columns} sortable>
                      <DataGridHeader>
                        <DataGridRow>
                          {({ renderHeaderCell }) => (
                            <DataGridHeaderCell>{renderHeaderCell()}</DataGridHeaderCell>
                          )}
                        </DataGridRow>
                      </DataGridHeader>
                      <DataGridBody<PreviewTransaction>>
                        {({ item, rowId }) => (
                          <DataGridRow key={rowId}>
                            {({ renderCell }) => <DataGridCell>{renderCell(item)}</DataGridCell>}
                          </DataGridRow>
                        )}
                      </DataGridBody>
                    </DataGrid>
                  </div>
                )}
              </>
            )}

            <div className={styles.actions}>
              <Button
                appearance="secondary"
                onClick={() => void doPreview(a.account)}
                disabled={s.busy || s.importing}
              >
                {s.preview ? "Re-preview" : "Preview"}
              </Button>
              {s.preview && s.preview.new.length > 0 && (
                <Button
                  appearance="primary"
                  onClick={() => void doImport(a.account, false)}
                  disabled={s.importing}
                >
                  {s.importing ? "Importing…" : `Import ${s.preview.new.length} transaction(s)`}
                </Button>
              )}
            </div>
          </Card>
        );
      })}
      {accounts.every((a) => a.files.length === 0) && (
        <MessageBar intent="info">
          <MessageBarBody>
            The inbox <b>{workspace.in_dir}</b> has no matching exports. Drop files named like{" "}
            <code>&lt;account&gt;.csv</code> or <code>&lt;account&gt;-anything.xls</code>, then
            restart the app (or reopen the workspace).
          </MessageBarBody>
        </MessageBar>
      )}
    </div>
  );
}
