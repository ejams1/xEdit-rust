{ parity probe: v-slot-73 }
unit P_v_slot_73;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  s: string;
begin
  s := 'abc';
  v := Ord(s[1]);
  Show('Ord(s[1])', v);
end;
end.
