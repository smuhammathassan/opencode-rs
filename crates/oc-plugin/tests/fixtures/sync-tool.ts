import { tool } from "./tool.js"

export const SyncPlugin = async (_ctx) => {
  return {
    tool: {
      synctool: tool({
        description: "sync",
        args: {},
        execute(args) {
          return `Sync ${args.foo}!`
        },
      }),
    },
  }
}
