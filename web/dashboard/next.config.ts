import type { NextConfig } from "next";

const config: NextConfig = {
  output: "export",
  trailingSlash: true,
  poweredByHeader: false,
  generateBuildId: async () => "qualyx-console-v1",
};
export default config;
