{ parity probe: v-slot-24 }
unit P_v_slot_24;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Integer;
begin
  i := '1' + 5;
  v := i;
  Show('i', v);
end;
end.
