//! A request the API sheds at its request limit was not run. Each family client reports it as
//! its own typed `overloaded` failure with "retry later", never as a protocol violation or an
//! uncertain outcome. The served API runs one request at a time on its socket; a held wake
//! wait takes that one, so every other call is shed.
