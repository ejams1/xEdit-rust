{ parity probe: x-097 }
unit P_x_097;

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
  AddMessage(nil);
  AddMessage('after');
end;
end.
