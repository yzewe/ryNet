package main

import (
	"encoding/hex"
	"encoding/json"
	"net"
	"os"

	utls "github.com/refraction-networking/utls"
)

func wsALPN(u *utls.UConn) {
	for _, e := range u.Extensions {
		if a, ok := e.(*utls.ALPNExtension); ok {
			a.AlpnProtocols = []string{"http/1.1"}
		}
	}
}

func main() {
	out := map[string][]string{}
	for _, fp := range []struct {
		name string
		id   utls.ClientHelloID
		ws   bool
	}{
		{"chrome", utls.HelloChrome_133, false},
		{"firefox", utls.HelloFirefox_148, false},
		{"safari", utls.HelloSafari_26_3, false},
		{"ios", utls.HelloIOS_14, false},
		{"edge", utls.HelloEdge_85, false},
		{"qq", utls.HelloQQ_11_1, false},
		{"chrome_ws", utls.HelloChrome_133, true},
		{"chrome120", utls.HelloChrome_120, false},
		{"chrome131", utls.HelloChrome_131, false},
		{"firefox120", utls.HelloFirefox_120, false},
		{"ios13", utls.HelloIOS_13, false},
		{"edge106", utls.HelloEdge_106, false},
		{"360_11", utls.Hello360_11_0, false},
	} {
		for i := 0; i < 40; i++ {
			client, server := net.Pipe()
			u := utls.UClient(client, &utls.Config{ServerName: "www.example.com"}, fp.id)
			if err := u.BuildHandshakeState(); err != nil {
				panic(err)
			}
			if fp.ws {
				wsALPN(u)
				if err := u.BuildHandshakeState(); err != nil {
					panic(err)
				}
			}
			out[fp.name] = append(out[fp.name], hex.EncodeToString(u.HandshakeState.Hello.Raw))
			_ = client.Close()
			_ = server.Close()
		}
	}
	b, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		panic(err)
	}
	if err := os.WriteFile("../../tests/fixtures/utls_hellos.json", append(b, '\n'), 0644); err != nil {
		panic(err)
	}
}
