{ parity probe: v-slot-39 }
unit P_v_slot_39;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  sl: array[0..2] of Integer;
begin
  v := VarType(High(sl));
  Show('VarType(High(sl))', v);
end;
end.
