import type { Thread } from '@delta/model';
import { Button, Panel, Spinner, StatusDot, type DotTone } from '@delta/ui-kit';
import type { ConnectionStatus } from '@delta/api-client';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { ThreadTree } from './ThreadTree';

export interface NavigatorPaneProps {
  threads: Thread[];
}

const CONNECTION_TONE: Record<ConnectionStatus, DotTone> = {
  connecting: 'amber',
  open: 'green',
  closed: 'red',
};

/**
 * The left pane: the thread tree plus the permission notice, a running
 * indicator, and the live connection status.
 */
export function NavigatorPane({ threads }: NavigatorPaneProps) {
  const connection = useLiveStore((state) => state.connection);
  const permission = useLiveStore((state) => state.permission);
  const dismissPermission = useLiveStore((state) => state.dismissPermission);
  const hasInProgress = useLiveStore((state) =>
    state.pending.some((item) => item.status === 'in_progress'),
  );
  const setTerminalOpen = useNavStore((state) => state.setTerminalOpen);

  return (
    <Panel
      className="border-r border-slate-200"
      header={
        <div className="flex items-center justify-between">
          <span className="text-sm font-semibold text-slate-700">Threads</span>
          <StatusDot tone={CONNECTION_TONE[connection]} label={connection} />
        </div>
      }
      footer={
        hasInProgress ? <Spinner label="running" /> : undefined
      }
    >
      {permission && (
        <div className="space-y-1 border-b border-amber-200 bg-amber-50 px-3 py-2 text-xs">
          <p className="font-medium text-amber-800">
            Permission requested: {permission.toolName}
          </p>
          <p className="text-slate-600">
            Answer the prompt in the terminal.
          </p>
          <div className="flex gap-2">
            <Button size="sm" onClick={() => setTerminalOpen(true)}>
              Open terminal
            </Button>
            <Button size="sm" variant="ghost" onClick={dismissPermission}>
              Dismiss
            </Button>
          </div>
        </div>
      )}
      <ThreadTree threads={threads} />
    </Panel>
  );
}
