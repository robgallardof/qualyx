import type { Metadata } from "next";
import "./globals.css";
export const metadata: Metadata = {
  title: "Qualyx console",
  description: "Test history and evidence. An optional viewer for the Qualyx CLI and MCP server.",
  robots: { index: false, follow: false },
};
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return <html lang="en" suppressHydrationWarning><body>{children}</body></html>;
}
