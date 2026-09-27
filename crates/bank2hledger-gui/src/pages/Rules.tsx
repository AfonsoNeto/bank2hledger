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
import { listRulesFiles, FileEntry } from "../api";
import { openInExplorer } from "../App";
import { WorkspaceInfo } from "../api";

const useStyles = makeStyles({
  root: { display: "flex", flexDirection: "column", ...shorthands.gap("12px") },
  card: { ...shorthands.padding("16px"), display: "flex", flexDirection: "column", ...shorthands.gap("8px") },
  file: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    ...shorthands.gap("12px"),
  },
});

export function Rules({ workspace }: { workspace: WorkspaceInfo }) {
  const styles = useStyles();
  const [files, setFiles] = React.useState<FileEntry[] | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    listRulesFiles()
      .then(setFiles)
      .catch((e) => setError(String(e)));
  }, [workspace.config_path]);

  return (
    <div className={styles.root}>
      <Title2>Rules</Title2>
      <Text>
        Rules files are <b>standard hledger CSV rules</b> living in{" "}
        <b>{workspace.rules_dir}</b>. They are generated once per account and never rewritten —
        edit freely; every fix you add makes the next import smarter.
      </Text>
      {error && (
        <MessageBar intent="error">
          <MessageBarBody>{error}</MessageBarBody>
        </MessageBar>
      )}
      <Card className={styles.card}>
        {files === null ? (
          <Spinner size="tiny" />
        ) : files.length === 0 ? (
          <Text>Rules files are created on first import.</Text>
        ) : (
          files.map((f) => (
            <div key={f.path} className={styles.file}>
              <Text>{f.file_name}</Text>
              <Button size="small" onClick={() => openInExplorer(f.path)}>
                Open
              </Button>
            </div>
          ))
        )}
      </Card>
    </div>
  );
}
