import plugin from "opencode/plugin"

export default {
  id: "auth-fixture",
  server: async () => {
    return {
      auth: {
        provider: "fixture-provider",
        methods: [
          {
            type: "oauth",
            label: "Fixture OAuth",
            prompts: [
              {
                type: "text",
                key: "account",
                message: "Account",
                placeholder: "team",
                when: { key: "mode", op: "eq", value: "team" },
                validate: (value) => value === "ok" ? undefined : "account is invalid",
              },
              {
                type: "select",
                key: "region",
                message: "Region",
                options: [
                  { label: "US", value: "us", hint: "United States" },
                  { label: "EU", value: "eu" },
                ],
              },
            ],
            authorize: async (inputs) => ({
              url: "https://auth.example.test/authorize?account=" + inputs.account,
              method: "code",
              instructions: "Enter the fixture authorization code",
              callback: async (code) => ({
                type: "success",
                provider: "fixture-provider",
                refresh: "refresh-" + code,
                access: "access-" + code,
                expires: 123,
              }),
            }),
          },
        ],
      },
    }
  },
}
