import type { StateCreator } from 'zustand';
import type { ConnectionStatus } from '@delta/api-client';

/** The live channel's connection status (drives the reconnect indicator). */
export interface ConnectionSlice {
  connection: ConnectionStatus;
  setConnection: (status: ConnectionStatus) => void;
}

export const createConnectionSlice: StateCreator<
  ConnectionSlice,
  [],
  [],
  ConnectionSlice
> = (set) => ({
  connection: 'connecting',

  setConnection: (status) => set({ connection: status }),
});
