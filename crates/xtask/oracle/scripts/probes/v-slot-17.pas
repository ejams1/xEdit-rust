{ parity probe: v-slot-17 }
unit P_v_slot_17;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  s: string;
begin
  s := Null;
  v := s;
  Show('s', v);
end;
end.
