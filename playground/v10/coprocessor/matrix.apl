⍝ TCBP Matrix Coprocessor
⍝ TCBP: [opcode 1B][payload_len 4B LE][payload N B][CRC16 2B LE]
⍝ Matrix payload: [ndims 1B][dims nd×4B LE][data n×8B LE float64]

⍝ --- CRC16-CCITT (poly 0x1021, init 0xFFFF) ---

∇ r ← xor16 a b
  r ← 2 ⊥ ((16⍴2) ⊤ a) ≠ (16⍴2) ⊤ b
∇

∇ r ← shl16 a
  r ← 2 ⊥ (1 ↓ (16⍴2) ⊤ a), 0
∇

∇ r ← crc16 bytes; crc; i; j; b
  crc ← 65535
  i ← 0
  :While i < ⍴bytes
      b ← bytes[i+1]
      crc ← xor16 crc (2 ⊥ ((8⍴2) ⊤ b), 8⍴0)
      j ← 0
      :While j < 8
          :If 32768 ≤ crc
              crc ← xor16 (shl16 crc) 4129
          :Else
              crc ← shl16 crc
          :EndIf
          j ← j + 1
      :EndWhile
      i ← i + 1
  :EndWhile
  r ← crc
∇

⍝ --- LE integer helpers ---

∇ r ← decu2 v
  r ← 256 ⊥ ⌽ v
∇

∇ r ← encu2 v
  r ← ⌽ (2⍴256) ⊤ v
∇

∇ r ← decu4 v
  r ← 256 ⊥ ⌽ v
∇

∇ r ← encu4 v
  r ← ⌽ (4⍴256) ⊤ v
∇

⍝ --- Float64 helpers ---

∇ r ← decd8 b; full; s; e; m
  full ← 256 ⊥ ⌽ b
  s ← ⌊full ÷ 2*63
  e ← ⌊(full - s×2*63) ÷ 2*52
  m ← (2*52) | full
  :If e = 0
      r ← (¯1*s) × (2*(-1022)) × m ÷ 2*52
  :ElseIf e = 2047
      r ← (¯1*s) ÷ 0
  :Else
      r ← (¯1*s) × (2*(e-1023)) × (1 + m ÷ 2*52)
  :EndIf
∇

∇ r ← decd8_arr v; n; i
  n ← (⍴v) ÷ 8
  r ← n ⍴ 0
  i ← 0
  :While i < n
      r[i+1] ← decd8 v[8×i + 1 2 3 4 5 6 7 8]
      i ← i + 1
  :EndWhile
∇

∇ b ← encd8 x; s; a; e; f; ei; m
  b ← 8⍴0
  :If x = 0 ⋄ :Return ⋄ :EndIf
  s ← (×x)=¯1
  a ← |x
  :If a ≥ 1.7976931348623157E308
      b ← ⌽(8⍴256)⊤(s×2*63)+(2047×2*52)
      :Return
  :EndIf
  e ← ⌊(⍟a)÷(⍟2)
  f ← a ÷ 2*e
  :While f ≥ 2 ⋄ f ← f ÷ 2 ⋄ e ← e + 1 ⋄ :EndWhile
  :While f < 1 ⋄ f ← f × 2 ⋄ e ← e - 1 ⋄ :EndWhile
  ei ← e + 1023
  :If ei ≤ 0 ⋄ :Return ⋄ :EndIf
  m ← ⌊0.5 + (f-1)×2*52
  :If m ≥ 2*52
      m ← 0 ⋄ ei ← ei + 1
      :If ei ≥ 2047
          b ← ⌽(8⍴256)⊤(s×2*63)+(2047×2*52)
          :Return
      :EndIf
  :EndIf
  b ← ⌽(8⍴256)⊤(s×2*63)+(ei×2*52)+m
∇

∇ b ← encd8_arr v; n; i
  n ← ⍴v
  b ← (n × 8) ⍴ 0
  i ← 0
  :While i < n
      b[8×i + 1 2 3 4 5 6 7 8] ← encd8 v[i+1]
      i ← i + 1
  :EndWhile
∇

⍝ Handle one TCBP client request
∇ handle fd; hdr; op; plen; payload; crc_r; crc_c; nd; dims; n; rw; A; res; out; dbc; rp
  op ← 0
  :Trap 0
      hdr ← ⎕FIO[26] fd 5
      →(5 ≠ ⍴hdr) / errok
      op ← hdr[1]
      plen ← decu4 hdr[2 3 4 5]
      →(plen = 0) / errok
      payload ← ⎕FIO[26] fd plen
      →(plen ≠ ⍴payload) / errok
      crc_r ← ⎕FIO[26] fd 2
      →(2 ≠ ⍴crc_r) / errok
      crc_c ← crc16 hdr, payload
      →(crc_c ≠ decu2 crc_r) / errok
      nd ← payload[1]
      →(nd ≠ 2) / errok
      dims ← decu4¨ ↓ (nd, 4) ⍴ payload[1 + ⍳4×nd]
      →(dims[1] ≠ dims[2]) / errok
      n ← ×/ dims
      rw ← payload[2 + 4×nd + ⍳8×n]
      A ← (dims[1], dims[1]) ⍴ decd8_arr rw
      res ← A +.× A
      out ← encd8_arr , res
      dbc ← ∊ encu4¨ dims
      rp ← (nd), dbc, out
      hdr ← op, encu4 (⍴rp)
      crc_c ← crc16 hdr, rp
      ⎕FIO[27] fd, hdr, rp, encu2 crc_c
      →0
errok:
      hdr ← op, encu4 0
      crc_c ← crc16 hdr
      ⎕FIO[27] fd, hdr, encu2 crc_c
      →0
  :Else
      hdr ← op, encu4 0
      crc_c ← crc16 hdr
      ⎕FIO[27] fd, hdr, encu2 crc_c
  :EndTrap
∇

⍝ Main server loop
∇ server port; sock; client
  sock ← ⎕FIO[42] port
  ⍞ ← 'APL Matrix Coprocessor listening on port ', ⍕port
loop:
  client ← ⎕FIO[43] sock
  →(client < 0) / loop
  'handle client' ⎕EA '⎕FIO[27] client, ⍬'
  ⎕FIO[46] client
  →loop
∇

server 8444
)OFF
