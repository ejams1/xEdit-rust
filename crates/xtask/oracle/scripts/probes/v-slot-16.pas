{ parity probe: v-slot-16 }
unit P_v_slot_16;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  i: Int64;
begin
  i := 4294967296;
  v := i;
  Show('i', v);
end;
end.
