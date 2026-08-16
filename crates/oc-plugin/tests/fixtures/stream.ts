export default async ({ client }) => {
  client.global.event().on("message", async (event) => {
    console.log("stream:" + event.type)
  })
}
