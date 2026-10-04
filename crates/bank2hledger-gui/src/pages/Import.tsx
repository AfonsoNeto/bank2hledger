import React from "react";
import {
  Button,
  Card,
  Checkbox,
  createTableColumn,
  Dropdown,
  Option,
  Tooltip,
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
  dupWrap: {
    display: "flex",
    flexDirection: "column",
    ...shorthands.gap("8px"),
    ...shorthands.padding("8px"),
    ...shorthands.borderRadius("6px"),
    backgroundColor: tokens.colorPaletteYellowBackground2,
  },
  dupRow: { display: "flex", alignItems: "center", justifyContent: "space-between", ...shorthands.gap("12px") },
  dupPick: { minWidth: "420px" },
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
  /// Per flagged row (staged_key): the selected option index in the
  /// duplicate menu. 0 = "None — import as it is" (the default).
  choices: Record<string, number>;
};

/// Helper text for the duplicate-resolution checkbox, shown on hover/focus.
const resolveDuplicatesTooltip =
  "After Preview, list every transaction that looks like one already in " +
  "your journal. For each, pick the matching entry and it will be skipped " +
  "as a duplicate (remembered, never offered again), or pick 'None' to " +
  "import it unchanged. Use it when your journal has hand-entered history " +
  "that the bank exports would otherwise double-count.";

export function ImportPage({ workspace }: { workspace: WorkspaceInfo }) {
  const styles = useStyles();
  const [accounts, setAccounts] = React.useState<AccountInbox[] | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [states, setStates] = React.useState<Record<string, AccountState>>({});
  const [resolveDuplicates, setResolveDuplicates] = React.useState(false);

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
        choices: {},
      };
      return { ...prev, [account]: { ...base, ...patch } };
    });

  const doPreview = async (account: string) => {
    setState(account, { busy: true, result: null, error: null, choices: {} });
    try {
      setState(account, {
        preview: await previewImport(account, resolveDuplicates),
        busy: false,
      });
    } catch (e) {
      setState(account, { busy: false, error: String(e) });
    }
  };

  const toggleResolveDuplicates = (checked: boolean) => {
    setResolveDuplicates(checked);
    // Re-preview every account that already has one, so the duplicate
    // choices appear as soon as the rows are identified.
    if (states) {
      for (const [account, s] of Object.entries(states)) {
        if (s.preview && !s.busy && !s.importing) {
          void doPreviewWith(account, checked);
        }
      }
    }
  };

  const doPreviewWith = async (account: string, resolve: boolean) => {
    setState(account, { busy: true, result: null, error: null, choices: {} });
    try {
      setState(account, { preview: await previewImport(account, resolve), busy: false });
    } catch (e) {
      setState(account, { busy: false, error: String(e) });
    }
  };

  const doImport = async (account: string, dryRun: boolean) => {
    setState(account, { importing: true, error: null });
    try {
      const s = states[account];
      const skipKeys = resolveDuplicates
        ? (s?.preview?.duplicates ?? [])
            .filter((d) => (s?.choices[d.staged_key] ?? 0) > 0)
            .map((d) => d.staged_key)
        : [];
      const r = await runImport(account, dryRun, skipKeys);
      setState(account, {
        importing: false,
        result:
          r.new_count === 0 && r.skipped_as_duplicates === 0
            ? "Nothing new to import."
            : `Imported ${r.new_count} transaction(s) (${r.already_seen} already imported` +
              (r.skipped_as_duplicates > 0
                ? `, ${r.skipped_as_duplicates} skipped as duplicates`
                : "") +
              "). Now check Balances against your bank app, then git commit to approve.",
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
      <Tooltip content={resolveDuplicatesTooltip} relationship="label">
        <Checkbox
          label="Resolve possible duplicates"
          checked={resolveDuplicates}
          onChange={(_e, data) => toggleResolveDuplicates(!!data.checked)}
        />
      </Tooltip>
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
                {resolveDuplicates && s.preview.duplicates.length > 0 && (
                  <div className={styles.dupWrap}>
                    <Text weight="semibold">
                      Possible duplicates — pick the matching journal entry, or "None" to
                      import as it is:
                    </Text>
                    {s.preview.duplicates.map((d) => {
                      const selected = s.choices[d.staged_key] ?? 0;
                      return (
                        <div key={d.staged_key} className={styles.dupRow}>
                          <Text>
                            {d.date} {d.payee} {d.amount} {d.currency}
                          </Text>
                          <Dropdown
                            className={styles.dupPick}
                            value={
                              selected === 0
                                ? "None — import the new transaction as it is"
                                : `${d.candidates[selected - 1].date} ${d.candidates[selected - 1].payee} ${d.candidates[selected - 1].amount}${d.candidates[selected - 1].currency} (score ${d.candidates[selected - 1].score.toFixed(2)})`
                            }
                            selectedOptions={[String(selected)]}
                            onOptionSelect={(_e, data) =>
                              setStates((prev) => ({
                                ...prev,
                                [a.account]: {
                                  ...(prev[a.account] ?? {
                                    preview: null, busy: false, importing: false,
                                    result: null, error: null, choices: {},
                                  }),
                                  choices: {
                                    ...(prev[a.account]?.choices ?? {}),
                                    [d.staged_key]: Number(data.optionValue ?? "0"),
                                  },
                                },
                              }))
                            }
                          >
                            <Option value="0" text="None — import the new transaction as it is">
                              None — import the new transaction as it is
                            </Option>
                            {d.candidates.map((c, i) => (
                              <Option
                                key={i}
                                value={String(i + 1)}
                                text={`${c.date} ${c.payee} ${c.amount}${c.currency} (score ${c.score.toFixed(2)})`}
                              >
                                {c.date} {c.payee} {c.amount}
                                {c.currency} (score {c.score.toFixed(2)})
                              </Option>
                            ))}
                          </Dropdown>
                        </div>
                      );
                    })}
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
