import React from "react";
import {
  Button,
  Card,
  CardHeader,
  makeStyles,
  shorthands,
  Spinner,
  Text,
  Title2,
} from "@fluentui/react-components";
import {
  HomeRegular,
  ArrowImportRegular,
  DataUsageRegular,
  GridDotsRegular,
  SettingsRegular,
  InfoRegular,
  BookOpenRegular,
} from "@fluentui/react-icons";
import { loadWorkspace, initWorkspace, WorkspaceInfo, openPath } from "./api";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Home } from "./pages/Home";
import { ImportPage } from "./pages/Import";
import { StatusPage } from "./pages/Status";
import { Accounts } from "./pages/Accounts";
import { Rules } from "./pages/Rules";
import { SettingsPage } from "./pages/Settings";
import { About } from "./pages/About";

const useStyles = makeStyles({
  root: {
    display: "flex",
    height: "100%",
    backgroundColor: "transparent",
  },
  nav: {
    width: "280px",
    flexShrink: 0,
    display: "flex",
    flexDirection: "column",
    paddingTop: "16px",
    backgroundColor: "transparent",
  },
  navTitle: {
    ...shorthands.padding("4px", "20px", "12px"),
  },
  navItem: {
    display: "flex",
    alignItems: "center",
    ...shorthands.gap("12px"),
    ...shorthands.padding("8px", "12px"),
    ...shorthands.margin("2px", "10px"),
    ...shorthands.borderRadius("6px"),
    cursor: "pointer",
    position: "relative",
    userSelect: "none",
    "&:hover": { backgroundColor: "var(--colorNeutralBackground1Hover)" },
  },
  navItemActive: {
    backgroundColor: "var(--colorNeutralBackground3Selected)",
    "&:hover": { backgroundColor: "var(--colorNeutralBackground3Selected)" },
    // Settings-style selection pill on the left edge
    "&::before": {
      content: '""',
      position: "absolute",
      left: "0px",
      top: "8px",
      bottom: "8px",
      width: "3px",
      ...shorthands.borderRadius("2px"),
      backgroundColor: "var(--colorBrandBackground)",
    },
  },
  content: {
    flex: 1,
    overflowY: "auto",
    ...shorthands.padding("24px", "32px", "48px"),
  },
  onboardingWrap: {
    height: "100%",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
  },
});

export type PageId =
  | "home"
  | "import"
  | "status"
  | "accounts"
  | "rules"
  | "settings"
  | "about";

const NAV_ITEMS: { id: PageId; label: string; icon: React.ReactElement }[] = [
  { id: "home", label: "Home", icon: <HomeRegular /> },
  { id: "import", label: "Import", icon: <ArrowImportRegular /> },
  { id: "status", label: "Balances", icon: <DataUsageRegular /> },
  { id: "accounts", label: "Accounts", icon: <GridDotsRegular /> },
  { id: "rules", label: "Rules", icon: <BookOpenRegular /> },
  { id: "settings", label: "Settings", icon: <SettingsRegular /> },
  { id: "about", label: "About", icon: <InfoRegular /> },
];

export function App() {
  const styles = useStyles();
  const [workspace, setWorkspace] = React.useState<WorkspaceInfo | null>(null);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(true);
  const [page, setPage] = React.useState<PageId>("home");

  const reload = React.useCallback(async (path: string | null) => {
    setBusy(true);
    setLoadError(null);
    try {
      setWorkspace(await loadWorkspace(path));
    } catch (e) {
      setWorkspace(null);
      setLoadError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  React.useEffect(() => {
    void reload(null);
  }, [reload]);

  const createWorkspace = React.useCallback(async () => {
    const dir = await openDialog({ directory: true, title: "Choose a workspace folder" });
    if (typeof dir !== "string") return;
    try {
      setWorkspace(await initWorkspace(dir, false));
      setLoadError(null);
    } catch (e) {
      setLoadError(String(e));
    }
  }, []);

  const openExisting = React.useCallback(async () => {
    const file = await openDialog({
      multiple: false,
      filters: [{ name: "bank2hledger config", extensions: ["toml"] }],
    });
    if (typeof file !== "string") return;
    void reload(file);
  }, [reload]);

  if (busy) {
    return (
      <div className={styles.onboardingWrap}>
        <Spinner label="Loading workspace…" />
      </div>
    );
  }

  if (!workspace) {
    return (
      <div className={styles.onboardingWrap}>
        <Card style={{ width: 480 }}>
          <CardHeader header={<Title2 block>Welcome to bank2hledger</Title2>} />
          <Text block>
            Import bank transaction exports into your hledger journal — with dedup,
            categorization rules, and a review-before-approve workflow.
          </Text>
          {loadError && (
            <Text block style={{ marginTop: 8, color: "var(--colorPaletteRedForeground1)" }}>
              {loadError}
            </Text>
          )}
          <div style={{ display: "flex", gap: 8, marginTop: 16 }}>
            <Button appearance="primary" onClick={() => void createWorkspace()}>
              Create a new workspace
            </Button>
            <Button onClick={() => void openExisting()}>Open existing config…</Button>
          </div>
        </Card>
      </div>
    );
  }

  const pages: Record<PageId, React.ReactNode> = {
    home: <Home workspace={workspace} go={setPage} reload={() => void reload(null)} />,
    import: <ImportPage workspace={workspace} />,
    status: <StatusPage workspace={workspace} />,
    accounts: <Accounts workspace={workspace} />,
    rules: <Rules workspace={workspace} />,
    settings: <SettingsPage workspace={workspace} />,
    about: <About />,
  };

  return (
    <div className={styles.root}>
      <nav className={styles.nav}>
        <Title2 className={styles.navTitle}>bank2hledger</Title2>
        {NAV_ITEMS.map((item) => (
          <div
            key={item.id}
            className={`${styles.navItem} ${page === item.id ? styles.navItemActive : ""}`}
            onClick={() => setPage(item.id)}
          >
            {item.icon}
            <Text weight="semibold">{item.label}</Text>
          </div>
        ))}
        <div style={{ marginTop: "auto", padding: "12px 20px" }}>
          <Text size={200} style={{ opacity: 0.7, wordBreak: "break-all" }}>
            {workspace.config_path}
          </Text>
        </div>
      </nav>
      <main className={styles.content}>{pages[page]}</main>
    </div>
  );
}

export function openInExplorer(path: string) {
  void openPath(path).catch((e) => console.error(e));
}
