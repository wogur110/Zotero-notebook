// Mirrors core/src/models.rs (camelCase serde). Keep in sync.

export interface ZoteroStatus {
  running: boolean;
  pluginInstalled: boolean;
  pluginVersion: string | null;
  hint: string | null;
}

export interface Collection {
  key: string;
  name: string;
  parentKey: string | null;
}

export type LinkMode =
  | "imported_file"
  | "imported_url"
  | "linked_file"
  | "linked_url"
  | "other";

export interface Attachment {
  key: string;
  title: string;
  filename: string | null;
  contentType: string | null;
  linkMode: LinkMode;
  filePath: string | null;
}

export interface Item {
  key: string;
  title: string;
  itemType: string;
  creators: string[];
  year: number | null;
  publication: string | null;
  doi: string | null;
  url: string | null;
  abstractText: string | null;
  tags: string[];
  dateAdded: string | null;
  collectionKeys: string[];
  attachment: Attachment | null;
}

export interface Library {
  collections: Collection[];
  items: Item[];
  writable: boolean;
}

export const UNCLASSIFIED_COLLECTION = "Unclassified";

export type ProviderId = "gemini" | "anthropic";

export interface ClassificationProposal {
  itemKey: string;
  proposedPath: string[];
  isNewCollection: boolean;
  confidence: number;
  rationale: string;
}

export interface ClassificationDecision {
  itemKey: string;
  targetPath: string[];
  /** Extra memberships to drop with the move (audit flow). */
  removeCollectionKeys?: string[];
}

export interface AuditProposal {
  itemKey: string;
  currentPaths: string[][];
  currentKeys: string[];
  proposedPath: string[];
  isNewCollection: boolean;
  confidence: number;
  rationale: string;
}

export interface MoveResult {
  itemKey: string;
  ok: boolean;
  error: string | null;
  collectionKey: string | null;
  newFilePath: string | null;
}

/** What the summarization prompt was based on. */
export type SummarySource = "fulltext" | "abstract" | "metadata";

export interface StoredSummary {
  itemKey: string;
  summary: string;
  provider: string;
  model: string;
  createdAt: string;
  source: SummarySource;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export interface ChatDelta {
  itemKey: string;
  delta: string;
}

export interface AppSettings {
  defaultProvider: ProviderId;
  geminiModel: string;
  anthropicModel: string;
  zoteroBaseUrl: string;
  fileRoot: string | null;
}

export interface ProgressEvent {
  done: number;
  total: number;
  itemKey: string | null;
  state: "running" | "ok" | "error";
  message: string | null;
}
