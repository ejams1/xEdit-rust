{ parity probe: v-slot-75 }
unit P_v_slot_75;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  d: Double;
begin
  v := d;
  Show('d', v);
end;
end.
