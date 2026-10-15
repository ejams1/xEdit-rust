{ parity probe: x-335 }
unit P_x_335;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := TStringList.ClassName; AddMessage(v);
  AddMessage('after');
end;
end.
