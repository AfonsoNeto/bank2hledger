import {
  Button,
  Card,
  makeStyles,
  MessageBar,
  MessageBarBody,
  shorthands,
  Text,
  Title2,
  Title3,
} from "@fluentui/react-components";
import { openInExplorer } from "../App";
import { WorkspaceInfo } from "../api";

const useStyles = makeStyles({
  root: { display: "flex", flexDirection: "column", ...shorthands.gap("12px") },
  card: { ...shorthands.padding("16px"), display: "flex", flexDirection: "column", ...shorthands.gap("8px") },
  row: { display: "flex", justifyContent: "space-between", alignItems: "center", ...shorthands.gap("12px") },
  path: { wordBreak: "break-all" },
});

function PathRow({ label, path }: { label: string; path: string }) {
  return (
    <div className={useStyles().row}>
      <div>
        <Text weight="semibold">{label}</Text>
        <br />
        <Text size={200} className={useStyles().path}>
          {path}
        </Text>
      </div>
      <Button size="small" onClick={() => openInExplorer(path)}>
        Open
      </Button>
    </div>
  );
}

export function SettingsPage({ workspace }: { workspace: WorkspaceInfo }) {
  const styles = useStyles();
  return (
    <div className={styles.root}>
      <Title2>Settings</Title2>
      <MessageBar intent="info">
        <MessageBarBody>
          All settings live in the TOML config file — edit it to change them, then reopen the
          workspace.
        </MessageBarBody>
      </MessageBar>

      <Card className={styles.card}>
        <Title3>Paths</Title3>
        <PathRow label="Config file" path={workspace.config_path} />
        <PathRow label="Journal" path={workspace.journal} />
        <PathRow label="Inbox (bank exports)" path={workspace.in_dir} />
        <PathRow label="Staging" path={workspace.staging_dir} />
        <PathRow label="Rules" path={workspace.rules_dir} />
      </Card>

      <Card className={styles.card}>
        <Title3>hledger</Title3>
        {workspace.hledger_version ? (
          <Text>{workspace.hledger_version}</Text>
        ) : (
          <MessageBar intent="warning">
            <MessageBarBody>
              hledger was not found on PATH. Install it (e.g.{" "}
              <code>winget install hledger.hledger</code>) or set the <code>HLEDGER</code>{" "}
              environment variable to its full path, then restart the app.
            </MessageBarBody>
          </MessageBar>
        )}
        <Text size={200}>
          Override the binary location with the <code>HLEDGER</code> environment variable.
        </Text>
      </Card>
    </div>
  );
}
