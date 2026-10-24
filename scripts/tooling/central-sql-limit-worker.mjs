// Permanent local probe: measure the SQLite surface exposed by a real DO.
export class SqlLimitProbe {
  constructor(state) {
    this.state = state;
  }

  async fetch() {
    const observations = [];
    for (let terms = 1; terms <= 16; terms += 1) {
      const sql = Array.from({ length: terms }, (_, index) => `SELECT ${index} AS ordinal`).join(" UNION ALL ");
      try {
        const rows = this.state.storage.sql.exec(sql).toArray();
        if (rows.length !== terms || rows.some((row, index) => row.ordinal !== index)) {
          throw new Error("compound statement returned different rows");
        }
        observations.push({ terms, accepted: true, rows: rows.length });
      } catch (error) {
        observations.push({ terms, accepted: false, error: String(error) });
      }
    }
    return Response.json(observations);
  }
}

export default {
  fetch(request, environment) {
    return environment.SQL_LIMIT_PROBE.get(environment.SQL_LIMIT_PROBE.idFromName("compound-select")).fetch(request);
  },
};
