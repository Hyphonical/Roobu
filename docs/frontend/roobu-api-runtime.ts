import type { operations } from "./roobu-api-client";

export type SearchResponse =
	operations["search"]["responses"][200]["content"]["application/json"];
export type SearchSimilarResponse =
	operations["search_similar"]["responses"][200]["content"]["application/json"];
export type SearchUploadResponse =
	operations["search_upload"]["responses"][200]["content"]["application/json"];
export type RecentResponse =
	operations["recent"]["responses"][200]["content"]["application/json"];
export type PostResponse =
	operations["get_post"]["responses"][200]["content"]["application/json"];
export type ActivityResponse =
	operations["activity"]["responses"][200]["content"]["application/json"];
export type SitesResponse =
	operations["sites"]["responses"][200]["content"]["application/json"];
export type IngestStatusResponse =
	operations["ingest_status"]["responses"][200]["content"]["application/json"];

export interface SearchParams {
	q?: string;
	image_url?: string;
	site?: string[];
	limit?: number;
	image_weight?: number;
}

export interface SearchUploadParams {
	q?: string;
	image?: Blob;
	site?: string[];
	limit?: number;
	image_weight?: number;
}

export interface SimilarParams {
	limit?: number;
}

export interface RecentParams {
	limit?: number;
	offset?: number;
	site?: string[];
}

export interface ActivityParams {
	days?: number;
	site?: string[];
}

export class RoobuApiError extends Error {
	readonly status: number;
	readonly body: unknown;

	constructor(status: number, body: unknown, message?: string) {
		super(message ?? `Roobu API request failed with status ${status}`);
		this.name = "RoobuApiError";
		this.status = status;
		this.body = body;
	}
}

export interface RoobuClientOptions {
	baseUrl: string;
	fetchImpl?: typeof fetch;
}

export class RoobuClient {
	private readonly baseUrl: string;
	private readonly fetchImpl: typeof fetch;

	constructor(options: RoobuClientOptions) {
		this.baseUrl = options.baseUrl.replace(/\/$/, "");
		this.fetchImpl = options.fetchImpl ?? fetch;
	}

	async search(params: SearchParams): Promise<SearchResponse> {
		const query = buildQuery({
			q: params.q,
			image_url: params.image_url,
			limit: params.limit,
			image_weight: params.image_weight,
			site: params.site,
		});
		return this.requestJson<SearchResponse>(`/api/search${query}`);
	}

	async searchUpload(params: SearchUploadParams): Promise<SearchUploadResponse> {
		const form = new FormData();
		if (params.q && params.q.trim().length > 0) {
			form.append("q", params.q.trim());
		}
		if (params.image) {
			form.append("image", params.image);
		}
		if (params.site) {
			for (const site of params.site) {
				form.append("site", site);
			}
		}
		if (typeof params.limit === "number") {
			form.append("limit", String(params.limit));
		}
		if (typeof params.image_weight === "number") {
			form.append("image_weight", String(params.image_weight));
		}

		return this.requestJson<SearchUploadResponse>("/api/search/upload", {
			method: "POST",
			body: form,
		});
	}

	async searchSimilar(
		site: string,
		postId: number,
		params: SimilarParams = {},
	): Promise<SearchSimilarResponse> {
		const query = buildQuery({ limit: params.limit });
		return this.requestJson<SearchSimilarResponse>(
			`/api/search/similar/${encodeURIComponent(site)}/${postId}${query}`,
		);
	}

	async recent(params: RecentParams = {}): Promise<RecentResponse> {
		const query = buildQuery({
			limit: params.limit,
			offset: params.offset,
			site: params.site,
		});
		return this.requestJson<RecentResponse>(`/api/recent${query}`);
	}

	async post(site: string, postId: number): Promise<PostResponse> {
		return this.requestJson<PostResponse>(
			`/api/post/${encodeURIComponent(site)}/${postId}`,
		);
	}

	async activity(params: ActivityParams = {}): Promise<ActivityResponse> {
		const query = buildQuery({ days: params.days, site: params.site });
		return this.requestJson<ActivityResponse>(`/api/activity${query}`);
	}

	async sites(): Promise<SitesResponse> {
		return this.requestJson<SitesResponse>("/api/sites");
	}

	async ingestStatus(): Promise<IngestStatusResponse> {
		return this.requestJson<IngestStatusResponse>("/api/ingest/status");
	}

	ingestWsUrl(): string {
		const httpBase = new URL(this.baseUrl);
		httpBase.protocol = httpBase.protocol === "https:" ? "wss:" : "ws:";
		httpBase.pathname = "/api/ws/ingest";
		httpBase.search = "";
		return httpBase.toString();
	}

	private async requestJson<T>(path: string, init?: RequestInit): Promise<T> {
		const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
			headers: {
				Accept: "application/json",
				...(init?.headers ?? {}),
			},
			...init,
		});

		const contentType = response.headers.get("content-type") ?? "";
		const isJson = contentType.includes("application/json");
		const body = isJson ? await response.json() : await response.text();

		if (!response.ok) {
			throw new RoobuApiError(response.status, body);
		}

		return body as T;
	}
}

function buildQuery(params: Record<string, unknown>): string {
	const query = new URLSearchParams();

	for (const [key, rawValue] of Object.entries(params)) {
		if (rawValue === undefined || rawValue === null) {
			continue;
		}

		if (Array.isArray(rawValue)) {
			for (const value of rawValue) {
				if (value !== undefined && value !== null) {
					query.append(key, String(value));
				}
			}
			continue;
		}

		if (typeof rawValue === "string" && rawValue.trim().length === 0) {
			continue;
		}

		query.set(key, String(rawValue));
	}

	const encoded = query.toString();
	return encoded.length > 0 ? `?${encoded}` : "";
}

export type IngestWsEvent =
	| {
			type: "connected";
			data: { message: string };
	  }
	| {
			type: "lagged";
			data: { dropped: number };
	  }
	| {
			type: "StatusSnapshot";
			data: {
				is_running: boolean;
				active_sites: string[];
				last_checkpoint?: unknown;
			};
	  }
	| {
			type: "CycleComplete";
			data: {
				site: string;
				fetched: number;
				upserted: number;
				elapsed_secs: number;
			};
	  }
	| {
			type: "CycleFailed";
			data: {
				site: string;
				error: string;
				elapsed_secs: number;
			};
	  }
	| {
			type: "CheckpointUpdated";
			data: {
				site: string;
				last_id: number;
			};
	  }
	| {
			type: "Sleeping";
			data: {
				site: string;
				sleep_secs: number;
			};
	  };
