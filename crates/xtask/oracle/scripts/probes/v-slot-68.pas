{ parity probe: v-slot-68 }
unit P_v_slot_68;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function F: Integer;
begin
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := F;
  Show('F', v);
end;
end.
