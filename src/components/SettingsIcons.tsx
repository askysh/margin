import type { ReactNode } from "react";

/** 16px line icons for the settings nav. One stroke weight, so the column reads as a set. */
function Icon({ children }: { children: ReactNode }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      {children}
    </svg>
  );
}

export const SettingsIcons = {
  general: () => (
    <Icon>
      <path d="M2.5 4.5h7M12.5 4.5h1M2.5 11.5h1M6.5 11.5h7" />
      <circle cx="11" cy="4.5" r="1.5" />
      <circle cx="5" cy="11.5" r="1.5" />
    </Icon>
  ),
  recording: () => (
    <Icon>
      <rect x="5.5" y="1.75" width="5" height="8" rx="2.5" />
      <path d="M3.25 7.75a4.75 4.75 0 009.5 0M8 12.5v1.75" />
    </Icon>
  ),
  transcription: () => (
    <Icon>
      <path d="M2.5 4h11M2.5 7h11M2.5 10h7M2.5 13h4.5" />
    </Icon>
  ),
  summaries: () => (
    <Icon>
      <path d="M8 1.75l1.5 3.25 3.25 1.5-3.25 1.5L8 11.25 6.5 8 3.25 6.5 6.5 5z" />
      <path d="M12.5 11.5l.6 1.4 1.4.6-1.4.6-.6 1.4-.6-1.4-1.4-.6 1.4-.6z" />
    </Icon>
  ),
  voices: () => (
    <Icon>
      <circle cx="6" cy="5.5" r="2.5" />
      <path d="M1.75 13.75c.5-2.25 2.2-3.5 4.25-3.5s3.75 1.25 4.25 3.5M11.5 4a3 3 0 010 4M13.25 2.5a5.25 5.25 0 010 7" />
    </Icon>
  ),
  permissions: () => (
    <Icon>
      <path d="M8 1.75l5 1.75v4c0 3.25-2.25 5.5-5 6.75-2.75-1.25-5-3.5-5-6.75v-4z" />
      <path d="M5.75 8l1.6 1.6 2.9-3.1" />
    </Icon>
  ),
  integrations: () => (
    <Icon>
      <path d="M6.5 9.5l3-3M7 4.5l1.25-1.25a2.65 2.65 0 013.75 3.75L10.75 8.25M9 11.5l-1.25 1.25A2.65 2.65 0 014 9l1.25-1.25" />
    </Icon>
  ),
  updates: () => (
    <Icon>
      <circle cx="8" cy="8" r="6.25" />
      <path d="M8 4.75v6M5.5 8.5L8 11l2.5-2.5" />
    </Icon>
  ),
  about: () => (
    <Icon>
      <circle cx="8" cy="8" r="6.25" />
      <path d="M8 7.25v4M8 4.9v.1" />
    </Icon>
  ),
  reset: () => (
    <Icon>
      <path d="M2.75 3.25v3.25H6" />
      <path d="M3.1 6.5A5.25 5.25 0 118 13.25a5.24 5.24 0 01-4.6-2.75" />
    </Icon>
  ),
  back: () => (
    <Icon>
      <path d="M9.75 3.5L5.25 8l4.5 4.5" />
    </Icon>
  ),
};
