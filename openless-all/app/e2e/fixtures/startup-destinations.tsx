import React from 'react';

// Isolate destination pages while keeping the production App startup effects intact.
export function FloatingShell() {
  return (
    <main>
      <h1>Startup reached the app</h1>
    </main>
  );
}
export function Onboarding() {
  return (
    <main>
      <h1>Startup reached the guide</h1>
    </main>
  );
}
export function GlobalDownloadProgress() {
  return null;
}
