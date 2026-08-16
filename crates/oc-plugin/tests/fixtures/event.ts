export default async () => ({
  event: async ({ event }) => {
    await Promise.resolve()
    console.log("received:" + event.type)
  },
})
