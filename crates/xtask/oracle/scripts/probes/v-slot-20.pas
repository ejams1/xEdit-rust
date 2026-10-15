{ parity probe: v-slot-20 }
unit P_v_slot_20;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  d: Double;
begin
  d := 'x';
  v := d;
  Show('d', v);
end;
end.
