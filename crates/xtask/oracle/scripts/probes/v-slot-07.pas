{ parity probe: v-slot-07 }
unit P_v_slot_07;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Integer;
begin
  i := 1.6;
  v := i;
  Show('i', v);
end;
end.
