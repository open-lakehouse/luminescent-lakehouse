import { type FakeFactory, seedQueue } from "@open-lakehouse/factory-client/testing";
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

/** Runs queued for a place: `sandbox` takes one run at a time, the demo's
 * running task holds it, and two tasks wait in line (their cards and the
 * inspector show where and how far back). */
export const QueuedRuns: StoryObj<typeof FactoryControlRoom> = {
  parameters: {
    factory: {
      setup: (fake: FakeFactory) => seedQueue(fake, [...fake.graphs.keys()][0] as string),
    },
  },
};
