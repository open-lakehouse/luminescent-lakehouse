import type { StorybookConfig } from "@storybook/react-vite";

const config: StorybookConfig = {
  stories: ["../../../packages/factory-ui/src/**/*.stories.tsx"],
  framework: { name: "@storybook/react-vite", options: {} },
};

export default config;
