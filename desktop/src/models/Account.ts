export interface ServerInfo {
  id: string;       // IS the server URL
  name: string;
  url: string;
  displayHost: string;
}

export interface Account {
  id: string;       // IS the DID
  displayName: string;
  avatarData: Uint8Array | null;
  servers: ServerInfo[];
}
