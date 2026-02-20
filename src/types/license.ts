export type LicensePlan = 'monthly' | 'yearly' | 'per_event';

export type LicenseUiState = 'active' | 'expired' | 'not_yet_valid' | 'invalid_device';

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
}

export interface LicenseRequestSummaryDto {
  path: string;
  archive_path: string;
  created_at: number;
  plan: LicensePlan;
  device_hash_hex: string;
  nonce_hex: string;
}

export type LicenseInputPayload =
  | { type: 'bytes'; bytes: number[] }
  | { type: 'path'; path: string };

export interface CommandError {
  code: string;
  message: string;
}
