import {
  Button,
  Card,
  makeStyles,
  shorthands,
  Text,
  Title2,
} from "@fluentui/react-components";
import { openUrl } from "@tauri-apps/plugin-opener";

const useStyles = makeStyles({
  root: { display: "flex", flexDirection: "column", ...shorthands.gap("12px"), maxWidth: "560px" },
  card: { ...shorthands.padding("16px"), display: "flex", flexDirection: "column", ...shorthands.gap("8px") },
});

export function About() {
  const styles = useStyles();
  return (
    <div className={styles.root}>
      <Title2>About</Title2>
      <Card className={styles.card}>
        <Text weight="semibold">bank2hledger GUI 0.1.0</Text>
        <Text>
          Import bank transaction exports (CSV/XLS/PDF) into your hledger journal with dedup,
          categorization rules, and a review-before-approve workflow.
        </Text>
        <Text size={200}>MIT licensed. Uses the same core library as the bank2hledger CLI.</Text>
        <div style={{ display: "flex", gap: 8 }}>
          <Button
            size="small"
            onClick={() => void openUrl("https://github.com/AfonsoNeto/bank2hledger")}
          >
            Project home
          </Button>
          <Button
            size="small"
            onClick={() => void openUrl("https://hledger.org/hledger.html#csv-rules")}
          >
            hledger CSV rules
          </Button>
        </div>
      </Card>
    </div>
  );
}
