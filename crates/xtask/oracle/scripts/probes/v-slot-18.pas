{ parity probe: v-slot-18 }
unit P_v_slot_18;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Integer;
begin
  i := Null;
  v := i;
  Show('i', v);
end;
end.
