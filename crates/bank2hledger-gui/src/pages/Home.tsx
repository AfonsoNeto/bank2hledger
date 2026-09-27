import {
  Badge,
  Card,
  makeStyles,
  MessageBar,
  MessageBarBody,
  shorthands,
  Text,
  Title2,
  Title3,
} from "@fluentui/react-components";
import {
  ArrowImportRegular,
  CheckmarkCircleRegular,
  DataUsageRegular,
  DismissCircleRegular,
} from "@fluentui/react-icons";
import { WorkspaceInfo } from "../api";
import { openInExplorer, PageId } from "../App";

const useStyles = makeStyles({
  hero: { ...shorthands.margin("0", "0", "16px") },
  grid: {
    display: "grid",
    gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))",
    ...shorthands.gap("12px"),
  },
  card: { ...shorthands.padding("16px"), display: "flex", flexDirection: "column", ...shorthands.gap("8px") },
  action: { display: "flex", alignItems: "center", ...shorthands.gap("8px"), cursor: "pointer", marginTop: "4px" },
  path: { wordBreak: "break-all", opacity: 0.8 },
});

export function Home({ workspace, go }: { workspace: WorkspaceInfo; go: (p: PageId) => void; reload: () => void }) {
  const styles = useStyles();
  return (
    <>
      <Title2 className={styles.hero}>Home</Title2>
      {!workspace.hledger_version && (
        <MessageBar intent="warning" style={{ marginBottom: 12 }}>
          <MessageBarBody>
            <b>hledger was not found.</b> Install it (e.g.{" "}
            <code>winget install hledger.hledger</code>) or set the <code>HLEDGER</code>{" "}
            environment variable, then restart the app.
          </MessageBarBody>
        </MessageBar>
      )}
      <div className={styles.grid}>
        <Card className={styles.card}>
          <Title3>Import transactions</Title3>
          <Text>
            Drop bank exports into <b>{workspace.in_dir}</b>, preview what is new, and import
            into your journal.
          </Text>
          <div className={styles.action} onClick={() => go("import")}>
            <ArrowImportRegular /> <Text weight="semibold">Go to Import</Text>
          </div>
        </Card>

        <Card className={styles.card}>
          <Title3>Check balances</Title3>
          <Text>
            Compare <code>hledger bal</code> against your bank apps, then git-commit the
            journal to approve the import.
          </Text>
          <div className={styles.action} onClick={() => go("status")}>
            <DataUsageRegular /> <Text weight="semibold">Go to Balances</Text>
          </div>
        </Card>

        <Card className={styles.card}>
          <Title3>Workspace</Title3>
          <Text className={styles.path}>Journal: {workspace.journal}</Text>
          <Text className={styles.path}>Exports: {workspace.in_dir}</Text>
          <Text className={styles.path}>Config: {workspace.config_path}</Text>
          <div className={styles.action} onClick={() => openInExplorer(workspace.config_path)}>
            <Text weight="semibold">Open config location</Text>
          </div>
        </Card>

        <Card className={styles.card}>
          <Title3>Setup</Title3>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            {workspace.hledger_version ? (
              <CheckmarkCircleRegular style={{ color: "green" }} />
            ) : (
              <DismissCircleRegular style={{ color: "red" }} />
            )}
            <Text>{workspace.hledger_version ?? "hledger: not found"}</Text>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            {workspace.accounts.length > 0 && <CheckmarkCircleRegular style={{ color: "green" }} />}
            <Text>
              {workspace.accounts.length} account(s) configured
            </Text>
          </div>
          <div>
            {workspace.accounts.map((a) => (
              <Badge key={a.name} appearance="outline" style={{ margin: 2 }}>
                {a.name} · {a.profile}
              </Badge>
            ))}
          </div>
          <div className={styles.action} onClick={() => go("accounts")}>
            <Text weight="semibold">Manage accounts</Text>
          </div>
        </Card>
      </div>
    </>
  );
}
