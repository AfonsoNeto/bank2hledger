import {
  Card,
  createTableColumn,
  DataGrid,
  DataGridBody,
  DataGridCell,
  DataGridHeader,
  DataGridHeaderCell,
  DataGridRow,
  makeStyles,
  shorthands,
  TableColumnDefinition,
  Text,
  Title2,
} from "@fluentui/react-components";
import { AccountInfo, WorkspaceInfo } from "../api";

const useStyles = makeStyles({
  root: { display: "flex", flexDirection: "column", ...shorthands.gap("12px") },
  card: { ...shorthands.padding("12px") },
});

const columns: TableColumnDefinition<AccountInfo>[] = [
  createTableColumn<AccountInfo>({
    columnId: "name",
    compare: (a, b) => a.name.localeCompare(b.name),
    renderHeaderCell: () => "Account",
    renderCell: (item) => item.name,
  }),
  createTableColumn<AccountInfo>({
    columnId: "profile",
    compare: (a, b) => a.profile.localeCompare(b.profile),
    renderHeaderCell: () => "Profile",
    renderCell: (item) => item.profile,
  }),
  createTableColumn<AccountInfo>({
    columnId: "hledger_account",
    compare: (a, b) => a.hledger_account.localeCompare(b.hledger_account),
    renderHeaderCell: () => "hledger account",
    renderCell: (item) => item.hledger_account,
  }),
  createTableColumn<AccountInfo>({
    columnId: "rules",
    compare: (a, b) => Number(!!b.rules_file) - Number(!!a.rules_file),
    renderHeaderCell: () => "Rules file",
    renderCell: (item) => (item.rules_file ? "yes" : "not yet generated"),
  }),
];

export function Accounts({ workspace }: { workspace: WorkspaceInfo }) {
  const styles = useStyles();
  return (
    <div className={styles.root}>
      <Title2>Accounts</Title2>
      <Text>
        Accounts are defined in <b>{workspace.config_path}</b> — edit the file to add, remove,
        or rename them.
      </Text>
      <Card className={styles.card}>
        <DataGrid
          items={workspace.accounts}
          columns={columns}
          sortable
          style={{ minWidth: "560px" }}
        >
          <DataGridHeader>
            <DataGridRow>
              {({ renderHeaderCell }) => (
                <DataGridHeaderCell>{renderHeaderCell()}</DataGridHeaderCell>
              )}
            </DataGridRow>
          </DataGridHeader>
          <DataGridBody<AccountInfo>>
            {({ item, rowId }) => (
              <DataGridRow key={rowId}>
                {({ renderCell }) => <DataGridCell>{renderCell(item)}</DataGridCell>}
              </DataGridRow>
            )}
          </DataGridBody>
        </DataGrid>
      </Card>
    </div>
  );
}
