export interface SearchResult {
  post_id: number;
  thumbnail_url: string;
  post_url: string;
  site: string;
  rating: "safe" | "questionable" | "explicit" | "";
}

export interface SearchResponse {
  type: "results";
  posts: SearchResult[];
  hasMore: boolean;
}

export interface DomainsResponse {
  type: "domains";
  domains: string[];
}

export interface SearchRequest {
  type: "search";
  query: string;
  domain: string;
  limit: number;
  offset: number;
}

export interface DomainsRequest {
  type: "domains";
}

export interface ErrorResponse {
  type: "error";
  error: string;
}

export type WSMessage = SearchResponse | DomainsResponse | ErrorResponse;
export type WSRequest = SearchRequest | DomainsRequest;

export type WebSocketStatus = "connecting" | "connected" | "disconnected" | "error";