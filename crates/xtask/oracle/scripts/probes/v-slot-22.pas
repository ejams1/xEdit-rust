{ parity probe: v-slot-22 }
unit P_v_slot_22;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  s: string;
begin
  s := '1';
  v := s + 1;
  Show('s + 1', v);
end;
end.
