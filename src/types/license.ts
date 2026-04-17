export type LicensePlan = 'monthly' | 'yearly' | 'per_event';

export type LicenseUiState =
  | 'active'
  | 'expired'
  | 'not_yet_valid'
  | 'device_mismatch'
  | 'missing'
  | 'invalid';

export interface LicenseStatusDto {
  status: LicenseUiState;
  plan?: string | null;
  customer_name?: string | null;
  license_id: string;
  not_before: number;
  not_after: number;
  max_clock_skew: number;
  device_hash_hex: string;
  installed_at: number;
  last_verified_at: number;
  last_checked_at: number;
  is_placeholder: boolean;
}

export interface LicenseRequestSummaryDto {
  exported_path: string;
  archived_path?: string;
  archived_internally: boolean;
  created_at: number;
  plan: LicensePlan;
  device_hash_hex: string;
  request_id_hex: string;
  installation_id: string;
  /** Temporary legacy alias kept for backend compatibility; do not use as canonical ID. */
  nonce_hex?: string;
}

export type LicenseInputPayload =
  | { type: 'bytes'; bytes: number[] }
  | { type: 'path'; path: string };

export interface CommandError {
  code: string;
  message: string;
}
