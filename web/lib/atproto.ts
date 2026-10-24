import { invoke } from '@tauri-apps/api/core';

export interface AtprotoLoginStart {
  url: string;
}

export interface AtprotoStatus {
  did: string;
  scope?: string;
  expires_at?: string;
}

export interface AtprotoProfileView {
  did: string;
  handle?: string;
  display_name?: string;
  description?: string;
  avatar_cid?: string;
}

export function atprotoStatus() {
  return invoke<Option<AtprotoStatus>>('atproto_status');
}

export function beginAtprotoLogin(handle: string) {
  return invoke<AtprotoLoginStart>('atproto_begin_login', { handle });
}

export function completeAtprotoLogin(callbackUrl: string) {
  return invoke<AtprotoStatus>('atproto_complete_login', { callbackUrl });
}

export function atprotoLogout() {
  return invoke('atproto_logout');
}

export function atprotoGetProfile() {
  return invoke<Option<AtprotoProfileView>>('atproto_get_profile');
}

export function atprotoErrorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as Record<string, unknown>).message;
    if (typeof message === "string") return message;
