import type { Meta, StoryObj } from "@storybook/react-vite";
import { FactoryControlRoom } from "./FactoryControlRoom.js";

const meta: Meta<typeof FactoryControlRoom> = {
  title: "Factory/Control room",
  component: FactoryControlRoom,
};
export default meta;

/** The demo plan: approved, then amended by an executor and the planner. */
export const Demo: StoryObj<typeof FactoryControlRoom> = {};

export const ReadOnly: StoryObj<typeof FactoryControlRoom> = { args: { readOnly: true } };
