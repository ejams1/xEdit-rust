{ parity probe: v-slot-77 }
unit P_v_slot_77;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  s: string;
begin
  v := s;
  Show('s', v);
end;
end.
