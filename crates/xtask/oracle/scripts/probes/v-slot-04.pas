{ parity probe: v-slot-04 }
unit P_v_slot_04;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  d: Double;
begin
  d := '1.5';
  v := d;
  Show('d', v);
end;
end.
