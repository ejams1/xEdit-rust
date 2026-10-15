{ parity probe: v-slot-56 }
unit P_v_slot_56;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(GetElementNativeValues(FileByIndex(0), 'File Header\HEDR\Number of Records'));
  Show('VarType(GetElementNativeValues(FileByIndex(0), ''File Header\HEDR\Number of Records''))', v);
end;
end.
