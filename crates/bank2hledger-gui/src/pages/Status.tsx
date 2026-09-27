import React from "react";
import {
  Button,
  Card,
  makeStyles,
  MessageBar,
  MessageBarBody,
  shorthands,
  Spinner,
  Text,
  Title2,
} from "@fluentui/react-components";
import { getStatus, WorkspaceInfo } from "../api";

const useStyles = makeStyles({
  root: { display: "flex", flexDirection: "column", ...shorthands.gap("12px") },
  output: {
    fontFamily: "Cascadia Mono, Consolas, monospace",
    fontSize: "13px",
    whiteSpace: "pre-wrap",
    ...shorthands.padding("12px"),
    ...shorthands.borderRadius("6px"),
    backgroundColor: "var(--colorNeutralBackground3)",
  },
});

export function StatusPage({ workspace }: { workspace: WorkspaceInfo }) {
  const styles = useStyles();
  const [output, setOutput] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const refresh = React.useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setOutput(await getStatus());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  React.useEffect(() => {
    void refresh();
  }, [refresh, workspace.config_path]);

  return (
    <div className={styles.root}>
      <Title2>Balances</Title2>
      <Text>
        Current <code>hledger bal</code> for your configured accounts — compare these against
        your bank apps, then git-commit the journal to approve imports.
      </Text>
      {error && (
        <MessageBar intent="error">
          <MessageBarBody>{error}</MessageBarBody>
        </MessageBar>
      )}
      <Card>
        {busy ? (
          <Spinner size="tiny" label="Running hledger…" />
        ) : (
          <pre className={styles.output}>{output ?? "(no output)"}</pre>
        )}
      </Card>
      <div>
        <Button appearance="primary" onClick={() => void refresh()} disabled={busy}>
          Refresh balances
        </Button>
      </div>
    </div>
  );
}
