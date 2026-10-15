{ parity probe: v-slot-72 }
unit P_v_slot_72;

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
  v := Ord(s);
  Show('Ord(s)', v);
end;
end.
