{ parity probe: v-slot-59 }
unit P_v_slot_59;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Sqr(2));
  Show('VarType(Sqr(2))', v);
end;
end.
